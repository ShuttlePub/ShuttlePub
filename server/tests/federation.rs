//! `POST /inbox` (Follow and Create) and `GET /actor`.
//!
//! The local application runs in-process, assembled the way `main` assembles
//! it: over PostgreSQL (a database of its own per test, see
//! `support::postgres`), with the Accept delivery saving its position in
//! PostgreSQL transactions. The remote service is a real HTTP server on
//! a loopback port, because the application resolves actors and delivers
//! Accepts over HTTP. An Accept is delivered by a reaction to the recorded
//! federation, after the Follow has been answered, so tests wait for it with a
//! deadline.
//!
//! Test names carry the issue's check numbers: `f01_`..`f12_` are the base
//! commit's Follow checks, `r01_`..`r09_` and `n01_`..`n09_` are this issue's
//! checks for Follow and Create. `order_` tests pin which answer wins when one
//! request trips several checks.

mod support;

use std::{
    collections::BTreeSet,
    error::Error,
    ops::Deref,
    sync::LazyLock,
    time::{Duration, SystemTime},
};

use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use http_body_util::{BodyExt, Full};
use http_msgsign_draft::{
    digest::Digest,
    errors::VerificationError,
    sign::{VerifierKey, headers::SignatureInput},
};
use rsa::{
    RsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
    signature::Verifier,
};
use serde_json::{Value, json};
use server::{
    app::{AppModule, Handler},
    database::PoolTransactions,
    delivery::{self, AcceptDeliveryHandle},
};
use sha2::Sha256;
use support::{
    activity::{
        LOCAL_ACTOR, LOCAL_HOST, PUBLIC, PUBLIC_BARE, PUBLIC_COMPACT, addressed, create, create_by,
        follow, note, note_by,
    },
    peer::{
        ActorReply, Peer, actor_document, padded_json, peer_serving, peer_with_broken_public_key,
        spawn_peer, unresponsive_peer, valid_peer,
    },
    postgres::TestDatabase,
    signing::{InboxRequest, REQUIRED_COMPONENTS, Sha256Hasher, Signing, sign, signed},
    wait::{REACTION_DEADLINE, eventually},
};
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn Error>>;

const ACTOR_ENV: &str = "SHUTTLEPUB_ACTOR";
const ORIGINS_ENV: &str = "SHUTTLEPUB_REMOTE_ORIGINS";
const BODY_LIMIT: usize = 262_144;
const ELSEWHERE: &str = "https://elsewhere.example";

/// Event families of the two aggregates, as the event type a journal row
/// carries starts with them.
const PRODUCER: &str = "shuttlepub.producer.v1";
const NOTE: &str = "shuttlepub.note.v1";

static LOCAL_KEY: LazyLock<Result<driver::keys::DevelopmentKey, String>> = LazyLock::new(|| {
    driver::keys::DevelopmentKey::generate().map_err(|error| format!("{error:?}"))
});

// ------------------------------------------------------- local application ----

/// The router of an application assembled the way `main` does. Dereferences
/// to the router, so a test sends requests to it directly; the database is
/// kept to read back what the application recorded, and `_reaction` keeps the
/// Accept delivery alive.
struct LocalApp {
    router: Router,
    database: TestDatabase,
    _reaction: AcceptDeliveryHandle,
}

impl Deref for LocalApp {
    type Target = Router;
    fn deref(&self) -> &Router {
        &self.router
    }
}

impl LocalApp {
    /// How many events of `family` the application has recorded.
    async fn recorded(&self, family: &str) -> Result<usize, Box<dyn Error>> {
        Ok(usize::try_from(self.database.recorded(family).await?)?)
    }

    async fn recorded_nothing(&self) -> Result<bool, Box<dyn Error>> {
        Ok(self.recorded(PRODUCER).await? == 0 && self.recorded(NOTE).await? == 0)
    }

    /// Waits until exactly `count` events of `family` are recorded.
    async fn wait_for_recorded(&self, family: &str, count: usize) -> TestResult {
        eventually(
            &format!("{count} {family} events"),
            REACTION_DEADLINE,
            async || Ok(self.recorded(family).await? == count),
        )
        .await
    }
}

/// Assembles the application the way `main` does, reading the settings from
/// `vars` instead of the process environment, and starts the Accept delivery.
async fn local_app(vars: &[(&str, &str)]) -> Result<LocalApp, String> {
    let config = driver::config::Config::from_lookup(|name: &str| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    })
    .map_err(|error| format!("{error:?}"))?;
    let key = LOCAL_KEY.as_ref().map_err(Clone::clone)?.clone();
    let database = TestDatabase::create()
        .await
        .map_err(|error| error.to_string())?;
    let pool = database
        .connect()
        .await
        .map_err(|error| error.to_string())?;
    let system = server::database::event_source_system(pool.clone())
        .await
        .map_err(|error| format!("{error:?}"))?;
    let handler = Handler::new(config, key, system).map_err(|error| format!("{error:?}"))?;
    let app = AppModule::new(handler);
    let reaction = delivery::spawn_accept_delivery(&app, PoolTransactions::new(pool)).await;
    Ok(LocalApp {
        router: server::router(app),
        database,
        _reaction: reaction,
    })
}

async fn app_for(peer: &Peer) -> Result<LocalApp, String> {
    local_app(&[
        (ACTOR_ENV, LOCAL_ACTOR),
        (ORIGINS_ENV, peer.origin.as_str()),
    ])
    .await
}

/// An application that allows every one of `peers`.
async fn app_for_all(peers: &[&Peer]) -> Result<LocalApp, String> {
    let origins = peers
        .iter()
        .map(|peer| peer.origin.as_str())
        .collect::<Vec<_>>()
        .join(",");
    local_app(&[(ACTOR_ENV, LOCAL_ACTOR), (ORIGINS_ENV, origins.as_str())]).await
}

/// An application that has already accepted federation from `peer`.
async fn accepted_app(peer: &Peer) -> Result<LocalApp, Box<dyn Error>> {
    let app = app_for(peer).await?;
    let status = post_inbox(&app, signed(peer, &follow(peer, 1)).await?).await?;
    assert_eq!(status, StatusCode::ACCEPTED, "Follow precondition");
    Ok(app)
}

async fn get_actor(app: &Router) -> Result<(StatusCode, HeaderMap, Value), Box<dyn Error>> {
    let response = app
        .clone()
        .oneshot(Request::get("/actor").body(Body::empty())?)
        .await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await?.to_bytes();
    Ok((status, headers, serde_json::from_slice(&body)?))
}

// ---------------------------------------------------------------- requests ----

async fn post_inbox(app: &Router, request: InboxRequest) -> Result<StatusCode, Box<dyn Error>> {
    let mut http = Request::post("/inbox").body(Body::from(request.body))?;
    *http.headers_mut() = request.headers;
    Ok(app.clone().oneshot(http).await?.status())
}

async fn send_inbox(
    app: &Router,
    method: Method,
    request: InboxRequest,
) -> Result<StatusCode, Box<dyn Error>> {
    let mut http = Request::builder()
        .method(method)
        .uri("/inbox")
        .body(Body::from(request.body))?;
    *http.headers_mut() = request.headers;
    Ok(app.clone().oneshot(http).await?.status())
}

fn signature_fields(headers: &HeaderMap) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    headers
        .get("signature")
        .ok_or("no Signature header")?
        .to_str()?
        .split(',')
        .map(|pair| {
            let (key, value) = pair.split_once('=').ok_or("Signature item without `=`")?;
            Ok((key.to_owned(), value.trim_matches('"').to_owned()))
        })
        .collect()
}

fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn with_signature(mut request: InboxRequest, value: &str) -> Result<InboxRequest, Box<dyn Error>> {
    request.headers.insert("signature", value.parse()?);
    Ok(request)
}

fn quoted(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{key}=\"{value}\""))
        .collect::<Vec<_>>()
        .join(",")
}

/// `request` with the signature bytes altered, every other value kept.
fn with_altered_signature(request: InboxRequest) -> Result<InboxRequest, Box<dyn Error>> {
    let fields = signature_fields(&request.headers)?;
    let mut signature = STANDARD.decode(field(&fields, "signature").ok_or("no signature")?)?;
    let last = signature.last_mut().ok_or("empty signature")?;
    *last ^= 0x01;
    let altered = fields
        .iter()
        .map(|(key, value)| {
            let value = if key == "signature" {
                STANDARD.encode(&signature)
            } else {
                value.clone()
            };
            (key.clone(), value)
        })
        .collect::<Vec<_>>();
    with_signature(request, &quoted(&altered))
}

struct LocalVerifier {
    id: String,
    key: VerifyingKey<Sha256>,
}

impl VerifierKey for LocalVerifier {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn algorithm(&self) -> String {
        "rsa-sha256".into()
    }

    fn verify(&self, target: &[u8], signature: &[u8]) -> Result<(), VerificationError> {
        let signature = Signature::try_from(signature)
            .map_err(|error| VerificationError::Crypto(Box::new(error)))?;
        self.key
            .verify(target, &signature)
            .map_err(|error| VerificationError::Crypto(Box::new(error)))
    }
}

fn assert_nothing_delivered(peer: &Peer) -> TestResult {
    assert!(peer.deliveries()?.is_empty(), "no Accept may be delivered");
    Ok(())
}

/// A public Create of the peer's Note `n`, signed by the peer's `/actor`.
async fn signed_create(peer: &Peer, n: u32) -> Result<InboxRequest, Box<dyn Error>> {
    signed(peer, &create(peer, n, &note(peer, n))).await
}

// ------------------------------------------------------------------ GET /actor ----

#[tokio::test]
async fn actor_document_is_served_as_activity_json_without_a_signature() -> TestResult {
    // Given an application configured with a local actor.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;

    // When the actor is requested without any signature.
    let (status, headers, actor) = get_actor(&app).await?;

    // Then the ActivityPub representation is returned.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/activity+json");
    assert_eq!(
        actor["@context"],
        json!([
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1"
        ])
    );
    assert_eq!(actor["type"], "Application");
    assert_eq!(actor["id"], LOCAL_ACTOR);
    assert_eq!(actor["inbox"], "https://local.example/inbox");
    assert_eq!(actor["publicKey"]["id"], format!("{LOCAL_ACTOR}#main-key"));
    assert_eq!(actor["publicKey"]["owner"], LOCAL_ACTOR);
    let pem = actor["publicKey"]["publicKeyPem"]
        .as_str()
        .ok_or("publicKeyPem must be a string")?;
    RsaPublicKey::from_public_key_pem(pem)?;
    Ok(())
}

// --------------------------------------------------------- /inbox methods ----

const NON_POST_METHODS: [Method; 2] = [Method::GET, Method::PUT];

#[tokio::test]
async fn inbox_answers_405_to_methods_other_than_post() -> TestResult {
    // Given an application with a POST-only inbox.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;

    for method in NON_POST_METHODS {
        // When the inbox is requested with another method and no signature.
        let request = InboxRequest {
            headers: HeaderMap::new(),
            body: Bytes::new(),
        };
        let status = send_inbox(&app, method.clone(), request).await?;

        // Then the method is refused before any signature check.
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} /inbox");
    }
    Ok(())
}

#[tokio::test]
async fn inbox_refuses_signed_follow_sent_with_another_method_without_contacting_the_remote()
-> TestResult {
    // Given a remote actor allowed by SHUTTLEPUB_REMOTE_ORIGINS.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let request = signed(&peer, &follow(&peer, 1)).await?;

    for method in NON_POST_METHODS {
        // When a well-formed signed Follow arrives with a method other than POST.
        let status = send_inbox(&app, method.clone(), request.clone()).await?;

        // Then it is refused with 405 without resolving the sender or answering it.
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} /inbox");
        assert_eq!(peer.actor_fetches(), 0, "{method} /inbox fetched the actor");
        assert_nothing_delivered(&peer)?;
    }
    Ok(())
}

// ------------------------------------------------------------------------ F1 ----

#[tokio::test]
async fn f01_signed_follow_is_answered_202_and_an_accept_reaches_the_remote_inbox() -> TestResult {
    // Given a remote actor allowed by SHUTTLEPUB_REMOTE_ORIGINS.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let follow = follow(&peer, 1);

    // When it posts a correctly signed Follow.
    let status = post_inbox(&app, signed(&peer, &follow).await?).await?;

    // Then 202 is returned and the Accept reaches the remote inbox in time.
    assert_eq!(status, StatusCode::ACCEPTED);
    peer.wait_for_deliveries(1).await?;
    let deliveries = peer.deliveries()?;
    assert_eq!(deliveries.len(), 1, "exactly one Accept per Follow");
    let accept: Value = serde_json::from_slice(&deliveries[0].body)?;
    assert_eq!(accept["@context"], "https://www.w3.org/ns/activitystreams");
    assert_eq!(accept["type"], "Accept");
    assert_eq!(accept["actor"], LOCAL_ACTOR);
    assert_eq!(accept["object"], follow);
    Ok(())
}

#[tokio::test]
async fn f01_delivered_accept_is_signed_by_the_published_local_key() -> TestResult {
    // Given a remote actor and the local actor's published public key.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let (_, _, actor) = get_actor(&app).await?;

    // When a signed Follow is accepted.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;
    assert_eq!(status, StatusCode::ACCEPTED);
    peer.wait_for_deliveries(1).await?;

    // Then the Accept carries the required headers, covers all five components
    // with rsa-sha256 under `{actor}#main-key`, and verifies with that key.
    let delivery = peer
        .deliveries()?
        .into_iter()
        .next()
        .ok_or("Accept was not delivered")?;
    assert_eq!(
        delivery.headers[header::CONTENT_TYPE],
        "application/activity+json"
    );
    assert_eq!(delivery.headers[header::HOST], peer.authority());
    httpdate::parse_http_date(delivery.headers[header::DATE].to_str()?)?;
    let fields = signature_fields(&delivery.headers)?;
    assert_eq!(field(&fields, "algorithm"), Some("rsa-sha256"));
    let key_id = format!("{LOCAL_ACTOR}#main-key");
    assert_eq!(field(&fields, "keyId"), Some(key_id.as_str()));
    let covered: BTreeSet<&str> = field(&fields, "headers")
        .ok_or("Signature without headers")?
        .split(' ')
        .collect();
    assert_eq!(covered, REQUIRED_COMPONENTS.into_iter().collect());
    let mut request = Request::post("/inbox").body(Full::new(delivery.body.clone()))?;
    *request.headers_mut() = delivery.headers.clone();
    let request = request
        .verify_digest::<Sha256Hasher>()
        .await
        .map_err(|error| format!("digest does not match the Accept body: {error:?}"))?;
    let input = SignatureInput::from_header(request.headers())
        .map_err(|error| format!("Signature header: {error:?}"))?;
    let pem = actor["publicKey"]["publicKeyPem"]
        .as_str()
        .ok_or("publicKeyPem must be a string")?;
    let verifier = LocalVerifier {
        id: key_id,
        key: VerifyingKey::new(RsaPublicKey::from_public_key_pem(pem)?),
    };
    input
        .verify_request(&request, &verifier)
        .map_err(|error| format!("Accept signature does not verify: {error:?}"))?;
    Ok(())
}

#[tokio::test]
async fn f01_each_follow_is_answered_with_its_own_accept_id() -> TestResult {
    // Given two different Follows from the same remote actor.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let first = follow(&peer, 1);
    let second = follow(&peer, 2);

    // When both are posted.
    assert_eq!(
        post_inbox(&app, signed(&peer, &first).await?).await?,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        post_inbox(&app, signed(&peer, &second).await?).await?,
        StatusCode::ACCEPTED
    );

    // Then each Accept answers its own Follow under a distinct id.
    peer.wait_for_deliveries(2).await?;
    let accepts = peer.accepts()?;
    assert_eq!(accepts.len(), 2);
    assert_eq!(accepts[0]["object"], first);
    assert_eq!(accepts[1]["object"], second);
    let prefix = format!("{LOCAL_ACTOR}/accepts/");
    for accept in &accepts {
        let id = accept["id"].as_str().ok_or("Accept id must be a string")?;
        assert!(id.starts_with(&prefix), "{id} is not under {prefix}");
    }
    assert_ne!(accepts[0]["id"], accepts[1]["id"]);
    Ok(())
}

// ------------------------------------------------------------------------ F2 ----

#[tokio::test]
async fn f02_body_over_256_kib_is_rejected_with_413() -> TestResult {
    // Given an otherwise valid signed Follow one byte over the limit.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let body = padded_json(&follow(&peer, 1), BODY_LIMIT + 1)?;

    // When it is posted.
    let status = post_inbox(&app, sign(body, &Signing::for_peer(&peer)).await?).await?;

    // Then it is refused before any remote work.
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(peer.actor_fetches(), 0);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f02_body_of_exactly_256_kib_is_still_accepted() -> TestResult {
    // Given a valid signed Follow padded to exactly the limit.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let body = padded_json(&follow(&peer, 1), BODY_LIMIT)?;

    // When it is posted.
    let status = post_inbox(&app, sign(body, &Signing::for_peer(&peer)).await?).await?;

    // Then the limit is inclusive.
    assert_eq!(status, StatusCode::ACCEPTED);
    peer.wait_for_deliveries(1).await?;
    assert_eq!(peer.deliveries()?.len(), 1);
    Ok(())
}

// ------------------------------------------------------------------------ F3 ----

#[tokio::test]
async fn f03_body_that_is_not_a_follow_is_rejected_with_400() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut wrong_type = follow(&peer, 1);
    wrong_type["type"] = json!("Note");
    let mut missing_actor = follow(&peer, 1);
    missing_actor
        .as_object_mut()
        .ok_or("Follow is an object")?
        .remove("actor");
    for (label, body) in [
        ("not JSON", b"{".to_vec()),
        ("not a Follow", serde_json::to_vec(&wrong_type)?),
        ("no actor", serde_json::to_vec(&missing_actor)?),
    ] {
        // Given a well-formed, matching signature over a body that is not a Follow.
        let request = sign(body, &Signing::for_peer(&peer)).await?;

        // When it is posted.
        let status = post_inbox(&app, request).await?;

        // Then the body is refused as a bad request.
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}");
    }
    assert_eq!(peer.actor_fetches(), 0);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f03_follow_id_that_is_not_a_url_is_rejected_with_400() -> TestResult {
    // Given a signed Follow whose id is not a URL.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut follow = follow(&peer, 1);
    follow["id"] = json!("not a url");

    // When it is posted.
    let status = post_inbox(&app, signed(&peer, &follow).await?).await?;

    // Then it is a bad request.
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f03_follow_actor_that_is_not_a_url_is_rejected_with_400() -> TestResult {
    // Given a signed Follow whose actor is not a URL.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut follow = follow(&peer, 1);
    follow["actor"] = json!("not a url");

    // When it is posted.
    let status = post_inbox(&app, signed(&peer, &follow).await?).await?;

    // Then it is a bad request and nothing is resolved.
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(peer.actor_fetches(), 0);
    assert_nothing_delivered(&peer)
}

// ------------------------------------------------------------------------ F4 ----

#[tokio::test]
async fn f04_missing_signature_header_is_rejected_with_401() -> TestResult {
    // Given a valid Follow without a Signature header.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut request = signed(&peer, &follow(&peer, 1)).await?;
    request.headers.remove("signature");

    // When it is posted.
    let status = post_inbox(&app, request).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f04_malformed_signature_header_is_rejected_with_401() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let valid = signed(&peer, &follow(&peer, 1)).await?;
    let fields = signature_fields(&valid.headers)?;
    let raw = quoted(&fields);
    let unquoted_algorithm = fields
        .iter()
        .map(|(key, value)| {
            if key == "algorithm" {
                format!("{key}={value}")
            } else {
                format!("{key}=\"{value}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let not_base64 = quoted(
        &fields
            .iter()
            .map(|(key, value)| {
                let value = if key == "signature" {
                    "not*base64*".to_owned()
                } else {
                    value.clone()
                };
                (key.clone(), value)
            })
            .collect::<Vec<_>>(),
    );
    for (label, header) in [
        // Each variant still carries the genuine, verifiable signature values,
        // so only the syntax check can reject it.
        (
            "duplicated item",
            format!("{raw},keyId=\"{}\"", peer.key_id()),
        ),
        ("unquoted value", unquoted_algorithm),
        ("signature is not Base64", not_base64),
    ] {
        // Given a Follow whose Signature header is malformed.
        let request = with_signature(valid.clone(), &header)?;

        // When it is posted.
        let status = post_inbox(&app, request).await?;

        // Then it is unauthorized.
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{label}");
    }
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f04_signature_that_does_not_match_is_rejected_with_401() -> TestResult {
    // Given a Follow whose signature bytes were altered.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let request = with_altered_signature(signed(&peer, &follow(&peer, 1)).await?)?;

    // When it is posted.
    let status = post_inbox(&app, request).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f04_body_changed_after_signing_is_rejected_with_401() -> TestResult {
    // Given the signature of one Follow sent with the body of another valid Follow.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut request = signed(&peer, &follow(&peer, 1)).await?;
    request.body = Bytes::from(serde_json::to_vec(&follow(&peer, 2))?);

    // When it is posted.
    let status = post_inbox(&app, request).await?;

    // Then the digest mismatch is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

// ------------------------------------------------------------------------ F5 ----

#[tokio::test]
async fn f05_signature_must_cover_all_five_required_components() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let body = serde_json::to_vec(&follow(&peer, 1))?;
    for omitted in REQUIRED_COMPONENTS {
        // Given a genuine signature that leaves one required component uncovered.
        let signing = Signing {
            covered: REQUIRED_COMPONENTS
                .into_iter()
                .filter(|component| *component != omitted)
                .collect(),
            ..Signing::for_peer(&peer)
        };
        let request = sign(body.clone(), &signing).await?;

        // When it is posted.
        let status = post_inbox(&app, request).await?;

        // Then it is unauthorized.
        assert_eq!(status, StatusCode::UNAUTHORIZED, "without {omitted}");
    }
    assert_nothing_delivered(&peer)
}

// ------------------------------------------------------------------------ F6 ----

#[tokio::test]
async fn f06_unparseable_date_is_rejected_with_401() -> TestResult {
    // Given a genuine signature over a Date that is not an HTTP date.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let signing = Signing {
        date: Some("yesterday".to_owned()),
        ..Signing::for_peer(&peer)
    };
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    // When it is posted.
    let status = post_inbox(&app, request).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f06_date_more_than_300_seconds_away_is_rejected_with_401() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let now = SystemTime::now();
    // 310 rather than 301 seconds: HTTP dates have one-second resolution and
    // the request is checked slightly after it is signed.
    let skew = Duration::from_secs(310);
    let past = now.checked_sub(skew).ok_or("clock before epoch")?;
    for (label, at) in [("future", now + skew), ("past", past)] {
        // Given a genuine signature over a Date outside the window.
        let signing = Signing::for_peer(&peer).dated(at);
        let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

        // When it is posted.
        let status = post_inbox(&app, request).await?;

        // Then it is unauthorized.
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{label}");
    }
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f06_date_within_300_seconds_is_accepted_in_both_directions() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let now = SystemTime::now();
    let skew = Duration::from_secs(250);
    let past = now.checked_sub(skew).ok_or("clock before epoch")?;
    for (label, at) in [("future", now + skew), ("past", past)] {
        // Given a genuine signature over a Date inside the window.
        let signing = Signing::for_peer(&peer).dated(at);
        let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

        // When it is posted.
        let status = post_inbox(&app, request).await?;

        // Then it is accepted.
        assert_eq!(status, StatusCode::ACCEPTED, "{label}");
    }
    Ok(())
}

#[tokio::test]
async fn f06_missing_date_is_rejected_with_401() -> TestResult {
    // Given a signature whose Date header was removed, from an actor whose
    // public key is unreadable: only a Date check that runs before the key is
    // parsed answers 401 here (a skipped Date check would surface as 502).
    let peer = peer_with_broken_public_key().await?;
    let app = app_for(&peer).await?;
    let mut request = signed(&peer, &follow(&peer, 1)).await?;
    request.headers.remove(header::DATE);

    // When it is posted.
    let status = post_inbox(&app, request).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

// ------------------------------------------------------------------------ F7 ----

#[tokio::test]
async fn f07_algorithm_other_than_rsa_sha256_is_rejected_with_401() -> TestResult {
    // Given a genuine RSA signature that declares another algorithm.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let signing = Signing {
        algorithm: "hs2019",
        ..Signing::for_peer(&peer)
    };
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    // When it is posted.
    let status = post_inbox(&app, request).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

// ------------------------------------------------------------------------ F8 ----

#[tokio::test]
async fn f08_key_id_other_than_the_actor_public_key_id_is_rejected_with_401() -> TestResult {
    // Given a signature by the actor's real key under a different keyId.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let signing = Signing {
        key_id: format!("{}#other-key", peer.actor_id()),
        ..Signing::for_peer(&peer)
    };
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    // When it is posted.
    let status = post_inbox(&app, request).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

// ------------------------------------------------------------------------ F9 ----

#[tokio::test]
async fn f09_fetched_actor_with_another_id_is_rejected_with_401() -> TestResult {
    // Given an actor document that names another actor (consistently with its key owner).
    let peer = peer_serving(|origin, document| {
        let other = format!("{origin}/someone-else");
        document["id"] = json!(other);
        document["publicKey"]["owner"] = json!(other);
    })
    .await?;
    let app = app_for(&peer).await?;

    // When a Follow signed for the requested actor is posted.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f09_public_key_owned_by_another_actor_is_rejected_with_401() -> TestResult {
    // Given an actor document whose public key is owned by someone else.
    let peer = peer_serving(|origin, document| {
        document["publicKey"]["owner"] = json!(format!("{origin}/someone-else"));
    })
    .await?;
    let app = app_for(&peer).await?;

    // When a correctly signed Follow is posted.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

// ----------------------------------------------------------------------- F10 ----

#[tokio::test]
async fn f10_follow_of_another_object_is_rejected_with_401() -> TestResult {
    // Given a signed Follow of an actor other than the local one.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut follow = follow(&peer, 1);
    follow["object"] = json!("https://local.example/someone-else");

    // When it is posted.
    let status = post_inbox(&app, signed(&peer, &follow).await?).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f10_host_other_than_the_local_actor_host_is_rejected_with_401() -> TestResult {
    // Given a Follow signed for a different Host.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let signing = Signing {
        host: "other.example".to_owned(),
        ..Signing::for_peer(&peer)
    };
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    // When it is posted.
    let status = post_inbox(&app, request).await?;

    // Then it is unauthorized.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f10_host_comparison_includes_the_port() -> TestResult {
    // Given a local actor on an explicit port.
    let peer = valid_peer().await?;
    let local = "https://local.example:8443/actor";
    let app = local_app(&[(ACTOR_ENV, local), (ORIGINS_ENV, peer.origin.as_str())]).await?;
    let mut follow = follow(&peer, 1);
    follow["object"] = json!(local);
    let body = serde_json::to_vec(&follow)?;
    let without_port = Signing {
        host: LOCAL_HOST.to_owned(),
        ..Signing::for_peer(&peer)
    };
    let with_port = Signing {
        host: "local.example:8443".to_owned(),
        ..Signing::for_peer(&peer)
    };

    // When the same Follow is signed for the host without and with the port.
    let rejected = post_inbox(&app, sign(body.clone(), &without_port).await?).await?;
    let accepted = post_inbox(&app, sign(body, &with_port).await?).await?;

    // Then only the exact host and port is accepted.
    assert_eq!(rejected, StatusCode::UNAUTHORIZED);
    assert_eq!(accepted, StatusCode::ACCEPTED);
    Ok(())
}

// ----------------------------------------------------------------------- F11 ----

#[tokio::test]
async fn f11_actor_on_an_unlisted_origin_is_rejected_with_403() -> TestResult {
    // Given an application that only allows another origin.
    let peer = valid_peer().await?;
    let app = local_app(&[(ACTOR_ENV, LOCAL_ACTOR), (ORIGINS_ENV, ELSEWHERE)]).await?;

    // When the peer posts a correctly signed Follow.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then it is forbidden without contacting the peer.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(peer.actor_fetches(), 0);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f11_actor_inbox_on_an_unlisted_origin_is_rejected_with_403() -> TestResult {
    // Given an allowed actor whose inbox lives on an unlisted origin.
    let peer = peer_serving(|_, document| {
        document["inbox"] = json!(format!("{ELSEWHERE}/inbox"));
    })
    .await?;
    let app = app_for(&peer).await?;

    // When it posts a correctly signed Follow.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then it is forbidden.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f11_actor_with_a_non_http_scheme_is_rejected_with_403() -> TestResult {
    // Given an actor URL whose scheme is not http/https, even though its
    // origin is listed.
    let peer = valid_peer().await?;
    let ftp_origin = format!("ftp://{}", peer.authority());
    let app = local_app(&[(ACTOR_ENV, LOCAL_ACTOR), (ORIGINS_ENV, ftp_origin.as_str())]).await?;
    let mut follow = follow(&peer, 1);
    follow["actor"] = json!(format!("{ftp_origin}/actor"));

    // When the Follow is posted.
    let status = post_inbox(&app, signed(&peer, &follow).await?).await?;

    // Then it is forbidden.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f11_actor_url_with_credentials_is_rejected_with_403() -> TestResult {
    // Given an actor URL on the allowed origin that carries credentials.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut follow = follow(&peer, 1);
    follow["actor"] = json!(format!("http://user:secret@{}/actor", peer.authority()));

    // When the Follow is posted.
    let status = post_inbox(&app, signed(&peer, &follow).await?).await?;

    // Then it is forbidden without contacting the peer.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(peer.actor_fetches(), 0);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f11_actor_inbox_with_credentials_is_rejected_with_403() -> TestResult {
    // Given an actor whose inbox URL carries credentials.
    let peer = peer_serving(|origin, document| {
        let authority = origin.trim_start_matches("http://");
        document["inbox"] = json!(format!("http://user:secret@{authority}/inbox"));
    })
    .await?;
    let app = app_for(&peer).await?;

    // When it posts a correctly signed Follow.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then it is forbidden.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_nothing_delivered(&peer)
}

// ----------------------------------------------------------------------- F12 ----

#[tokio::test]
async fn f12_actor_fetch_error_status_is_answered_502() -> TestResult {
    // Given a remote that fails to serve its actor.
    let peer = spawn_peer(
        |_| {
            Ok(ActorReply {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                location: None,
                body: b"{}".to_vec(),
            })
        },
        StatusCode::ACCEPTED,
    )
    .await?;
    let app = app_for(&peer).await?;

    // When it posts a correctly signed Follow.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then the remote failure is a bad gateway.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f12_actor_document_over_256_kib_is_answered_502() -> TestResult {
    // Given an otherwise valid actor document one byte over the limit.
    let peer = spawn_peer(
        |origin| ActorReply::padded(&actor_document(origin)?, BODY_LIMIT + 1),
        StatusCode::ACCEPTED,
    )
    .await?;
    let app = app_for(&peer).await?;

    // When a correctly signed Follow is posted.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then it is a bad gateway.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f12_actor_document_of_exactly_256_kib_is_still_accepted() -> TestResult {
    // Given a valid actor document padded to exactly the limit.
    let peer = spawn_peer(
        |origin| ActorReply::padded(&actor_document(origin)?, BODY_LIMIT),
        StatusCode::ACCEPTED,
    )
    .await?;
    let app = app_for(&peer).await?;

    // When a correctly signed Follow is posted.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then the limit is inclusive.
    assert_eq!(status, StatusCode::ACCEPTED);
    Ok(())
}

#[tokio::test]
async fn f12_unreadable_public_key_is_answered_502() -> TestResult {
    // Given an actor whose public key PEM cannot be read.
    let peer = peer_with_broken_public_key().await?;
    let app = app_for(&peer).await?;

    // When a correctly signed Follow is posted.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then it is a bad gateway.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_nothing_delivered(&peer)
}

// ------------------------------------------------------------------------ R3 ----

#[tokio::test]
async fn r03_accept_rejected_by_the_remote_inbox_is_recorded_and_resent_for_the_same_follow()
-> TestResult {
    // Given a remote inbox that answers every Accept with an error status.
    let peer = spawn_peer(
        |origin| Ok(ActorReply::json(&actor_document(origin)?)?),
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await?;
    let app = app_for(&peer).await?;
    let follow = follow(&peer, 1);

    // When a correctly signed Follow is posted.
    let status = post_inbox(&app, signed(&peer, &follow).await?).await?;

    // Then it is answered 202, the delivery is attempted, and its failure is
    // recorded next to the federation.
    assert_eq!(status, StatusCode::ACCEPTED);
    peer.wait_for_deliveries(1).await?;
    app.wait_for_recorded(PRODUCER, 2).await?;

    // When the remote sends the same Follow again.
    let status = post_inbox(&app, signed(&peer, &follow).await?).await?;

    // Then the federation is recorded again and the Accept is delivered again.
    assert_eq!(status, StatusCode::ACCEPTED);
    peer.wait_for_deliveries(2).await?;
    let accepts = peer.accepts()?;
    assert_eq!(accepts.len(), 2);
    assert_eq!(accepts[1]["object"], follow);
    app.wait_for_recorded(PRODUCER, 4).await
}

// ------------------------------------------------------------- remote client ----

#[tokio::test]
async fn actor_redirect_is_not_followed() -> TestResult {
    // Given a remote whose actor URL redirects to a valid actor document.
    let peer = spawn_peer(
        |origin| {
            Ok(ActorReply {
                status: StatusCode::FOUND,
                location: Some(format!("{origin}/redirect-target").parse()?),
                body: Vec::new(),
            })
        },
        StatusCode::ACCEPTED,
    )
    .await?;
    let app = app_for(&peer).await?;

    // When a correctly signed Follow is posted.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then the redirect is not followed and the empty answer is not an actor.
    assert_eq!(peer.redirect_fetches(), 0);
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_nothing_delivered(&peer)
}

// ---------------------------------------------------------------- settings ----

#[tokio::test]
async fn remote_origins_are_comma_separated_and_empty_entries_are_ignored() -> TestResult {
    // Given the peer listed among empty entries and with a path that is not
    // part of an origin.
    let peer = valid_peer().await?;
    let origins = format!(",{ELSEWHERE},,{}/ignored/path,", peer.origin);
    let app = local_app(&[(ACTOR_ENV, LOCAL_ACTOR), (ORIGINS_ENV, origins.as_str())]).await?;

    // When the peer posts a correctly signed Follow.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then the peer's origin is allowed.
    assert_eq!(status, StatusCode::ACCEPTED);
    Ok(())
}

// ------------------------------------------------------------------- order ----

#[tokio::test]
async fn order_oversized_body_wins_over_missing_signature() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut request = sign(
        padded_json(&follow(&peer, 1), BODY_LIMIT + 1)?,
        &Signing::for_peer(&peer),
    )
    .await?;
    request.headers.remove("signature");

    assert_eq!(
        post_inbox(&app, request).await?,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    Ok(())
}

#[tokio::test]
async fn order_missing_signature_wins_over_unparseable_body() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut request = sign(b"{".to_vec(), &Signing::for_peer(&peer)).await?;
    request.headers.remove("signature");

    assert_eq!(post_inbox(&app, request).await?, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn order_follow_id_check_wins_over_object_check() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut follow = follow(&peer, 1);
    follow["id"] = json!("not a url");
    follow["object"] = json!("https://local.example/someone-else");

    assert_eq!(
        post_inbox(&app, signed(&peer, &follow).await?).await?,
        StatusCode::BAD_REQUEST
    );
    Ok(())
}

#[tokio::test]
async fn order_object_check_wins_over_actor_url_check() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut follow = follow(&peer, 1);
    follow["object"] = json!("https://local.example/someone-else");
    follow["actor"] = json!("not a url");

    assert_eq!(
        post_inbox(&app, signed(&peer, &follow).await?).await?,
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}

#[tokio::test]
async fn order_host_check_wins_over_actor_url_check() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let mut follow = follow(&peer, 1);
    follow["actor"] = json!("not a url");
    let signing = Signing {
        host: "other.example".to_owned(),
        ..Signing::for_peer(&peer)
    };

    assert_eq!(
        post_inbox(&app, sign(serde_json::to_vec(&follow)?, &signing).await?).await?,
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}

#[tokio::test]
async fn order_host_check_wins_over_actor_origin_check() -> TestResult {
    let peer = valid_peer().await?;
    let app = local_app(&[(ACTOR_ENV, LOCAL_ACTOR), (ORIGINS_ENV, ELSEWHERE)]).await?;
    let signing = Signing {
        host: "other.example".to_owned(),
        ..Signing::for_peer(&peer)
    };
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    assert_eq!(post_inbox(&app, request).await?, StatusCode::UNAUTHORIZED);
    assert_eq!(peer.actor_fetches(), 0);
    Ok(())
}

#[tokio::test]
async fn order_actor_identity_check_wins_over_inbox_origin_check() -> TestResult {
    let peer = peer_serving(|origin, document| {
        let other = format!("{origin}/someone-else");
        document["id"] = json!(other);
        document["publicKey"]["owner"] = json!(other);
        document["inbox"] = json!(format!("{ELSEWHERE}/inbox"));
    })
    .await?;
    let app = app_for(&peer).await?;

    assert_eq!(
        post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?,
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}

#[tokio::test]
async fn order_inbox_origin_check_wins_over_key_id_check() -> TestResult {
    let peer = peer_serving(|_, document| {
        document["inbox"] = json!(format!("{ELSEWHERE}/inbox"));
    })
    .await?;
    let app = app_for(&peer).await?;
    let signing = Signing {
        key_id: format!("{}#other-key", peer.actor_id()),
        ..Signing::for_peer(&peer)
    };
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    assert_eq!(post_inbox(&app, request).await?, StatusCode::FORBIDDEN);
    Ok(())
}

#[tokio::test]
async fn order_actor_fetch_failure_wins_over_key_id_check() -> TestResult {
    let peer = spawn_peer(
        |_| {
            Ok(ActorReply {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                location: None,
                body: b"{}".to_vec(),
            })
        },
        StatusCode::ACCEPTED,
    )
    .await?;
    let app = app_for(&peer).await?;
    let signing = Signing {
        key_id: format!("{}#other-key", peer.actor_id()),
        ..Signing::for_peer(&peer)
    };
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    assert_eq!(post_inbox(&app, request).await?, StatusCode::BAD_GATEWAY);
    Ok(())
}

#[tokio::test]
async fn order_algorithm_check_wins_over_public_key_parsing() -> TestResult {
    let peer = peer_with_broken_public_key().await?;
    let app = app_for(&peer).await?;
    let signing = Signing {
        algorithm: "hs2019",
        ..Signing::for_peer(&peer)
    };
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    assert_eq!(post_inbox(&app, request).await?, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn order_date_check_wins_over_public_key_parsing() -> TestResult {
    let peer = peer_with_broken_public_key().await?;
    let app = app_for(&peer).await?;
    let signing = Signing::for_peer(&peer).dated(SystemTime::now() + Duration::from_secs(310));
    let request = sign(serde_json::to_vec(&follow(&peer, 1))?, &signing).await?;

    assert_eq!(post_inbox(&app, request).await?, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn order_public_key_parsing_wins_over_digest_check() -> TestResult {
    let peer = peer_with_broken_public_key().await?;
    let app = app_for(&peer).await?;
    let mut request = signed(&peer, &follow(&peer, 1)).await?;
    request.body = Bytes::from(serde_json::to_vec(&follow(&peer, 2))?);

    assert_eq!(post_inbox(&app, request).await?, StatusCode::BAD_GATEWAY);
    Ok(())
}

// ------------------------------------------------------------------------ R1 ----

#[tokio::test]
async fn r01_signed_follow_records_the_federation_and_then_the_delivered_accept() -> TestResult {
    // Given a remote actor allowed by SHUTTLEPUB_REMOTE_ORIGINS.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;

    // When it posts a correctly signed Follow.
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;

    // Then the federation is recorded by the time 202 is returned, and once
    // the Accept has reached the remote inbox its delivery is recorded too.
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(
        app.recorded(PRODUCER).await? >= 1,
        "federation not recorded"
    );
    peer.wait_for_deliveries(1).await?;
    app.wait_for_recorded(PRODUCER, 2).await?;
    assert_eq!(peer.deliveries()?.len(), 1);
    assert_eq!(app.recorded(NOTE).await?, 0);
    Ok(())
}

#[tokio::test]
async fn r01_follow_is_answered_without_waiting_for_the_accept_delivery() -> TestResult {
    // Given a remote whose inbox takes every Accept and never answers.
    let peer = unresponsive_peer().await?;
    let app = app_for(&peer).await?;

    // When it posts a correctly signed Follow.
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?),
    )
    .await;

    // Then 202 comes back well before the 10-second delivery timeout, and the
    // Accept is attempted afterwards.
    assert_eq!(
        response.map_err(|_| "the Follow response waited for the Accept delivery")??,
        StatusCode::ACCEPTED
    );
    peer.wait_for_deliveries(1).await
}

// ------------------------------------------------------------------------ R2 ----

#[tokio::test]
async fn r02_follow_refused_by_any_check_records_nothing_and_delivers_nothing() -> TestResult {
    let peer = valid_peer().await?;
    let body = serde_json::to_vec(&follow(&peer, 1))?;
    let mut not_a_follow = follow(&peer, 1);
    not_a_follow["type"] = json!("Note");
    let mut other_object = follow(&peer, 1);
    other_object["object"] = json!("https://local.example/someone-else");
    let mut missing_signature = signed(&peer, &follow(&peer, 1)).await?;
    missing_signature.headers.remove("signature");
    let skewed = SystemTime::now() + Duration::from_secs(310);
    let cases = [
        (
            "F2 body over the limit",
            sign(
                padded_json(&follow(&peer, 1), BODY_LIMIT + 1)?,
                &Signing::for_peer(&peer),
            )
            .await?,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            "F3 not a Follow",
            signed(&peer, &not_a_follow).await?,
            StatusCode::BAD_REQUEST,
        ),
        (
            "F4 no signature",
            missing_signature,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "F4 altered signature",
            with_altered_signature(signed(&peer, &follow(&peer, 1)).await?)?,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "F5 Date not covered",
            sign(
                body.clone(),
                &Signing {
                    covered: REQUIRED_COMPONENTS
                        .into_iter()
                        .filter(|component| *component != "date")
                        .collect(),
                    ..Signing::for_peer(&peer)
                },
            )
            .await?,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "F6 Date outside the window",
            sign(body.clone(), &Signing::for_peer(&peer).dated(skewed)).await?,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "F7 other algorithm",
            sign(
                body.clone(),
                &Signing {
                    algorithm: "hs2019",
                    ..Signing::for_peer(&peer)
                },
            )
            .await?,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "F8 other keyId",
            sign(
                body.clone(),
                &Signing {
                    key_id: format!("{}#other-key", peer.actor_id()),
                    ..Signing::for_peer(&peer)
                },
            )
            .await?,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "F10 other object",
            signed(&peer, &other_object).await?,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "F10 other Host",
            sign(
                body.clone(),
                &Signing {
                    host: "other.example".to_owned(),
                    ..Signing::for_peer(&peer)
                },
            )
            .await?,
            StatusCode::UNAUTHORIZED,
        ),
    ];
    for (label, request, expected) in cases {
        // Given a fresh application that allows the peer.
        let app = app_for(&peer).await?;

        // When a Follow that fails one check is posted.
        let status = post_inbox(&app, request).await?;

        // Then the answer is unchanged and nothing is recorded.
        assert_eq!(status, expected, "{label}");
        assert!(
            app.recorded_nothing().await?,
            "{label}: something was recorded"
        );
    }
    assert_nothing_delivered(&peer)?;

    // F9: the fetched actor names another actor.
    let impostor = peer_serving(|origin, document| {
        let other = format!("{origin}/someone-else");
        document["id"] = json!(other);
        document["publicKey"]["owner"] = json!(other);
    })
    .await?;
    let app = app_for(&impostor).await?;
    let status = post_inbox(&app, signed(&impostor, &follow(&impostor, 1)).await?).await?;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "F9");
    assert!(app.recorded_nothing().await?, "F9: something was recorded");
    assert_nothing_delivered(&impostor)?;

    // F11: the actor's origin is not allowed.
    let app = local_app(&[(ACTOR_ENV, LOCAL_ACTOR), (ORIGINS_ENV, ELSEWHERE)]).await?;
    let status = post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?;
    assert_eq!(status, StatusCode::FORBIDDEN, "F11");
    assert!(app.recorded_nothing().await?, "F11: something was recorded");
    assert_nothing_delivered(&peer)
}

// ------------------------------------------------------------------------ R4 ----

#[tokio::test]
async fn r04_follow_repeated_after_its_accept_was_delivered_records_and_delivers_nothing()
-> TestResult {
    // Given a Follow whose Accept has been delivered and recorded, and a
    // second service that lets the test see the reaction has moved past it.
    let peer = valid_peer().await?;
    let other = valid_peer().await?;
    let app = app_for_all(&[&peer, &other]).await?;
    let repeated = follow(&peer, 1);
    assert_eq!(
        post_inbox(&app, signed(&peer, &repeated).await?).await?,
        StatusCode::ACCEPTED
    );
    peer.wait_for_deliveries(1).await?;
    app.wait_for_recorded(PRODUCER, 2).await?;

    // When the same Follow arrives again.
    let status = post_inbox(&app, signed(&peer, &repeated).await?).await?;

    // Then it is answered 202 without a new event.
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(app.recorded(PRODUCER).await?, 2);

    // And once the reaction has delivered a later Accept to the other
    // service, the first service has still received only its one Accept.
    assert_eq!(
        post_inbox(&app, signed(&other, &follow(&other, 1)).await?).await?,
        StatusCode::ACCEPTED
    );
    other.wait_for_deliveries(1).await?;
    app.wait_for_recorded(PRODUCER, 4).await?;
    assert_eq!(peer.deliveries()?.len(), 1);
    Ok(())
}

// ------------------------------------------------------------------------ R5 ----

#[tokio::test]
async fn r05_follow_with_a_new_id_after_delivery_is_recorded_and_accepted_again() -> TestResult {
    // Given a Follow whose Accept has been delivered and recorded.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    assert_eq!(
        post_inbox(&app, signed(&peer, &follow(&peer, 1)).await?).await?,
        StatusCode::ACCEPTED
    );
    peer.wait_for_deliveries(1).await?;
    app.wait_for_recorded(PRODUCER, 2).await?;

    // When the same actor sends a Follow with another id.
    let second = follow(&peer, 2);
    let status = post_inbox(&app, signed(&peer, &second).await?).await?;

    // Then the federation is recorded again and that Follow gets its Accept.
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(
        app.recorded(PRODUCER).await? >= 3,
        "federation not recorded"
    );
    peer.wait_for_deliveries(2).await?;
    assert_eq!(peer.accepts()?[1]["object"], second);
    app.wait_for_recorded(PRODUCER, 4).await
}

// ------------------------------------------------------------------------ R9 ----

#[tokio::test]
async fn r09_unresponsive_remote_does_not_hold_back_the_accept_for_another_service() -> TestResult {
    // Given an Accept on its way to a remote inbox that never answers.
    let silent = unresponsive_peer().await?;
    let other = valid_peer().await?;
    let app = app_for_all(&[&silent, &other]).await?;
    assert_eq!(
        post_inbox(&app, signed(&silent, &follow(&silent, 1)).await?).await?,
        StatusCode::ACCEPTED
    );
    silent.wait_for_deliveries(1).await?;

    // When another service sends a Follow.
    let other_follow = follow(&other, 1);
    let status = post_inbox(&app, signed(&other, &other_follow).await?).await?;

    // Then its Accept still arrives in time.
    assert_eq!(status, StatusCode::ACCEPTED);
    other.wait_for_deliveries(1).await?;
    assert_eq!(other.accepts()?[0]["object"], other_follow);
    Ok(())
}

// ------------------------------------------------------------------------ N1 ----

#[tokio::test]
async fn n01_public_note_from_an_accepted_producer_is_recorded() -> TestResult {
    // Given a service whose federation was accepted.
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;

    // When it posts a correctly signed Create of a public Note.
    let status = post_inbox(&app, signed_create(&peer, 1).await?).await?;

    // Then one "note received" event is recorded.
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(app.recorded(NOTE).await?, 1);
    Ok(())
}

// ------------------------------------------------------------------------ N2 ----

#[tokio::test]
async fn n02_the_same_note_arriving_again_adds_no_event() -> TestResult {
    // Given a Note already recorded.
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let note = note(&peer, 1);
    let first = create(&peer, 1, &note);
    assert_eq!(
        post_inbox(&app, signed(&peer, &first).await?).await?,
        StatusCode::ACCEPTED
    );
    assert_eq!(app.recorded(NOTE).await?, 1);

    for (label, activity) in [
        ("the same Create", first.clone()),
        ("another Create of the same Note", create(&peer, 2, &note)),
    ] {
        // When the Note arrives again.
        let status = post_inbox(&app, signed(&peer, &activity).await?).await?;

        // Then it is accepted and the Note is still recorded once.
        assert_eq!(status, StatusCode::ACCEPTED, "{label}");
        assert_eq!(app.recorded(NOTE).await?, 1, "{label}");
    }
    Ok(())
}

// ------------------------------------------------------------------------ N3 ----

#[tokio::test]
async fn n03_create_without_urls_or_an_embedded_note_is_rejected_with_400() -> TestResult {
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let valid = create(&peer, 1, &note(&peer, 1));
    let mut create_id = valid.clone();
    create_id["id"] = json!("not a url");
    let mut actor = valid.clone();
    actor["actor"] = json!("not a url");
    let mut note_id = valid.clone();
    note_id["object"]["id"] = json!("not a url");
    let mut referenced = valid.clone();
    referenced["object"] = json!(format!("{}/notes/1", peer.origin));
    let mut article = valid.clone();
    article["object"]["type"] = json!("Article");
    let mut missing = valid.clone();
    missing
        .as_object_mut()
        .ok_or("Create is an object")?
        .remove("object");
    for (label, activity) in [
        ("Create id is not a URL", create_id),
        ("actor is not a URL", actor),
        ("Note id is not a URL", note_id),
        ("object is a reference, not an embedded Note", referenced),
        ("embedded object is not a Note", article),
        ("no object", missing),
    ] {
        // Given a correctly signed Create whose shape is wrong.
        let request = signed(&peer, &activity).await?;

        // When it is posted.
        let status = post_inbox(&app, request).await?;

        // Then it is a bad request and nothing is recorded.
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}");
        assert_eq!(app.recorded(NOTE).await?, 0, "{label}");
    }
    Ok(())
}

// ------------------------------------------------------------------------ N4 ----

#[tokio::test]
async fn n04_create_failing_the_signature_checks_is_answered_like_a_follow() -> TestResult {
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let body = serde_json::to_vec(&create(&peer, 1, &note(&peer, 1)))?;
    let mut missing_signature = signed_create(&peer, 1).await?;
    missing_signature.headers.remove("signature");
    let mut body_changed = signed_create(&peer, 1).await?;
    body_changed.body = Bytes::from(serde_json::to_vec(&create(&peer, 2, &note(&peer, 2)))?);
    let skewed = SystemTime::now() + Duration::from_secs(310);
    let cases = [
        ("F4 no signature", missing_signature),
        (
            "F4 altered signature",
            with_altered_signature(signed_create(&peer, 1).await?)?,
        ),
        ("F4 body changed after signing", body_changed),
        (
            "F5 Digest not covered",
            sign(
                body.clone(),
                &Signing {
                    covered: REQUIRED_COMPONENTS
                        .into_iter()
                        .filter(|component| *component != "digest")
                        .collect(),
                    ..Signing::for_peer(&peer)
                },
            )
            .await?,
        ),
        (
            "F6 Date outside the window",
            sign(body.clone(), &Signing::for_peer(&peer).dated(skewed)).await?,
        ),
        (
            "F7 other algorithm",
            sign(
                body.clone(),
                &Signing {
                    algorithm: "hs2019",
                    ..Signing::for_peer(&peer)
                },
            )
            .await?,
        ),
        (
            "F8 other keyId",
            sign(
                body.clone(),
                &Signing {
                    key_id: format!("{}#other-key", peer.actor_id()),
                    ..Signing::for_peer(&peer)
                },
            )
            .await?,
        ),
    ];
    for (label, request) in cases {
        // When a Create from the accepted producer fails a signature check.
        let status = post_inbox(&app, request).await?;

        // Then it is unauthorized, as a Follow would be, and nothing is recorded.
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{label}");
        assert_eq!(app.recorded(NOTE).await?, 0, "{label}");
    }
    Ok(())
}

#[tokio::test]
async fn n04_create_from_an_actor_with_an_unreadable_key_is_answered_502() -> TestResult {
    // Given an allowed actor whose public key PEM cannot be read.
    let peer = peer_with_broken_public_key().await?;
    let app = app_for(&peer).await?;

    // When it posts a correctly signed Create.
    let status = post_inbox(&app, signed_create(&peer, 1).await?).await?;

    // Then it is a bad gateway, as a Follow would be.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(app.recorded_nothing().await?);
    Ok(())
}

// ------------------------------------------------------------------------ N5 ----

#[tokio::test]
async fn n05_create_for_another_host_is_rejected_with_401() -> TestResult {
    // Given a Create from the accepted producer signed for a different Host.
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let signing = Signing {
        host: "other.example".to_owned(),
        ..Signing::for_peer(&peer)
    };
    let body = serde_json::to_vec(&create(&peer, 1, &note(&peer, 1)))?;

    // When it is posted.
    let status = post_inbox(&app, sign(body, &signing).await?).await?;

    // Then it is unauthorized and nothing is recorded.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.recorded(NOTE).await?, 0);
    Ok(())
}

// ------------------------------------------------------------------------ N6 ----

#[tokio::test]
async fn n06_create_from_an_unlisted_origin_is_rejected_with_403() -> TestResult {
    // Given an application that only allows another origin.
    let peer = valid_peer().await?;
    let app = local_app(&[(ACTOR_ENV, LOCAL_ACTOR), (ORIGINS_ENV, ELSEWHERE)]).await?;

    // When the peer posts a correctly signed Create.
    let status = post_inbox(&app, signed_create(&peer, 1).await?).await?;

    // Then it is forbidden without contacting the peer, as a Follow would be.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(peer.actor_fetches(), 0);
    assert!(app.recorded_nothing().await?);
    Ok(())
}

#[tokio::test]
async fn n06_create_whose_actor_cannot_be_fetched_is_answered_502() -> TestResult {
    // Given a remote that fails to serve its actor.
    let peer = spawn_peer(
        |_| {
            Ok(ActorReply {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                location: None,
                body: b"{}".to_vec(),
            })
        },
        StatusCode::ACCEPTED,
    )
    .await?;
    let app = app_for(&peer).await?;

    // When it posts a correctly signed Create.
    let status = post_inbox(&app, signed_create(&peer, 1).await?).await?;

    // Then it is a bad gateway, as a Follow would be.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(app.recorded_nothing().await?);
    Ok(())
}

// ------------------------------------------------------------------------ N7 ----

#[tokio::test]
async fn n07_note_attributed_to_someone_other_than_the_create_actor_is_rejected_with_401()
-> TestResult {
    // Given a Create by the accepted actor of a Note attributed to another
    // actor of the same service.
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let foreign = note_by(&peer, &peer.member_id(), 1);

    // When it is posted.
    let status = post_inbox(&app, signed(&peer, &create(&peer, 1, &foreign)).await?).await?;

    // Then it is unauthorized and nothing is recorded.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.recorded(NOTE).await?, 0);
    Ok(())
}

#[tokio::test]
async fn n07_note_id_on_another_origin_than_the_actor_is_rejected_with_401() -> TestResult {
    // Given a Create of a Note whose id lives on another origin.
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let mut foreign = note(&peer, 1);
    foreign["id"] = json!(format!("{ELSEWHERE}/notes/1"));

    // When it is posted.
    let status = post_inbox(&app, signed(&peer, &create(&peer, 1, &foreign)).await?).await?;

    // Then it is unauthorized and nothing is recorded.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.recorded(NOTE).await?, 0);
    Ok(())
}

// ------------------------------------------------------------------------ N8 ----

#[tokio::test]
async fn n08_create_from_a_service_whose_federation_was_not_accepted_is_rejected_with_403()
-> TestResult {
    // Given an allowed service that never sent a Follow.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;

    // When it posts a correctly signed public Create.
    let status = post_inbox(&app, signed_create(&peer, 1).await?).await?;

    // Then it is forbidden and nothing is recorded.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(app.recorded_nothing().await?);
    Ok(())
}

#[tokio::test]
async fn n08_create_by_another_actor_of_an_accepted_service_is_recorded() -> TestResult {
    // Given a service whose `/actor` sent the accepted Follow.
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;

    // When another actor of the same service posts a Create of its own Note,
    // signed with its own key.
    let member = peer.member_id();
    let activity = create_by(&peer, &member, 1, &note_by(&peer, &member, 1));
    let request = sign(serde_json::to_vec(&activity)?, &Signing::for_member(&peer)).await?;
    let status = post_inbox(&app, request).await?;

    // Then the service is recognised by its authority and the Note is recorded.
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(app.recorded(NOTE).await?, 1);
    Ok(())
}

#[tokio::test]
async fn n08_federation_of_one_service_does_not_admit_another() -> TestResult {
    // Given two allowed services, only the first of which sent a Follow.
    let accepted = valid_peer().await?;
    let stranger = valid_peer().await?;
    let app = app_for_all(&[&accepted, &stranger]).await?;
    assert_eq!(
        post_inbox(&app, signed(&accepted, &follow(&accepted, 1)).await?).await?,
        StatusCode::ACCEPTED
    );

    // When the other service posts a correctly signed public Create.
    let status = post_inbox(&app, signed_create(&stranger, 1).await?).await?;

    // Then it is forbidden and nothing is recorded.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(app.recorded(NOTE).await?, 0);
    Ok(())
}

// ------------------------------------------------------------------------ N9 ----

#[tokio::test]
async fn n09_note_not_addressed_to_the_public_is_accepted_without_being_recorded() -> TestResult {
    // Given a Create whose audience is the followers collection only.
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let followers = format!("{}/followers", peer.actor_id());
    let private = addressed(note(&peer, 1), json!([followers]), json!([]));

    // When it is posted.
    let status = post_inbox(&app, signed(&peer, &create(&peer, 1, &private)).await?).await?;

    // Then it is accepted but not recorded.
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(app.recorded(NOTE).await?, 0);
    Ok(())
}

#[tokio::test]
async fn n09_every_spelling_and_position_of_the_public_audience_is_recorded() -> TestResult {
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let followers = json!([format!("{}/followers", peer.actor_id())]);
    let audiences = [
        ("full IRI in to", json!([PUBLIC]), json!([])),
        ("as:Public in to", json!([PUBLIC_COMPACT]), json!([])),
        ("Public in to", json!([PUBLIC_BARE]), json!([])),
        ("full IRI in cc", followers.clone(), json!([PUBLIC])),
        ("to as a single string", json!(PUBLIC), followers.clone()),
    ];
    for (n, (label, to, cc)) in (1..).zip(audiences) {
        // Given a Create of a new Note addressed to the public this way.
        let addressed_note = addressed(note(&peer, n), to, cc);
        let request = signed(&peer, &create(&peer, n, &addressed_note)).await?;

        // When it is posted.
        let status = post_inbox(&app, request).await?;

        // Then the Note is recorded.
        assert_eq!(status, StatusCode::ACCEPTED, "{label}");
        assert_eq!(app.recorded(NOTE).await?, usize::try_from(n)?, "{label}");
    }
    Ok(())
}

// ------------------------------------------------------------- Create order ----

#[tokio::test]
async fn order_create_shape_check_wins_over_host_check() -> TestResult {
    let peer = valid_peer().await?;
    let app = accepted_app(&peer).await?;
    let mut activity = create(&peer, 1, &note(&peer, 1));
    activity["object"]["id"] = json!("not a url");
    let signing = Signing {
        host: "other.example".to_owned(),
        ..Signing::for_peer(&peer)
    };

    assert_eq!(
        post_inbox(&app, sign(serde_json::to_vec(&activity)?, &signing).await?).await?,
        StatusCode::BAD_REQUEST
    );
    Ok(())
}

#[tokio::test]
async fn order_create_signature_check_wins_over_federation_check() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let request = with_altered_signature(signed_create(&peer, 1).await?)?;

    assert_eq!(post_inbox(&app, request).await?, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn order_create_attribution_check_wins_over_federation_check() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let foreign = note_by(&peer, &peer.member_id(), 1);

    assert_eq!(
        post_inbox(&app, signed(&peer, &create(&peer, 1, &foreign)).await?).await?,
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}

#[tokio::test]
async fn order_create_federation_check_wins_over_audience_check() -> TestResult {
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let followers = format!("{}/followers", peer.actor_id());
    let private = addressed(note(&peer, 1), json!([followers]), json!([]));

    assert_eq!(
        post_inbox(&app, signed(&peer, &create(&peer, 1, &private)).await?).await?,
        StatusCode::FORBIDDEN
    );
    Ok(())
}
