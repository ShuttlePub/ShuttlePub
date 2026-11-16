//! Follow→Accept over `POST /inbox` and `GET /actor`.
//!
//! The local application runs in-process; the remote actor is a real HTTP
//! server on a loopback port, because the application resolves actors and
//! delivers Accepts over HTTP. Test names carry the issue's check numbers
//! (`f01_`..`f12_`); `order_` tests pin which answer wins when one request
//! trips several checks, following the base commit's evaluation order.

use std::{
    collections::BTreeSet,
    error::Error,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use http_body_util::{BodyExt, Full};
use http_msgsign_draft::{
    digest::{ContentHasher, Digest, DigestHash},
    errors::VerificationError,
    sign::{RequestSign, SignatureParams, SignerKey, VerifierKey, headers::SignatureInput},
};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::{Signature, SigningKey, VerifyingKey},
    pkcs8::{DecodePublicKey, EncodePublicKey, LineEnding},
    signature::{SignatureEncoding, Signer, Verifier},
};
use serde_json::{Value, json};
use sha2::Sha256;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn Error>>;

const LOCAL_ACTOR: &str = "https://local.example/actor";
const LOCAL_HOST: &str = "local.example";
const ACTOR_ENV: &str = "SHUTTLEPUB_ACTOR";
const ORIGINS_ENV: &str = "SHUTTLEPUB_REMOTE_ORIGINS";
const BODY_LIMIT: usize = 262_144;
const REQUIRED_COMPONENTS: [&str; 5] =
    ["(request-target)", "host", "date", "digest", "content-type"];
const ELSEWHERE: &str = "https://elsewhere.example";

static LOCAL_KEY: LazyLock<Result<driver::keys::DevelopmentKey, String>> = LazyLock::new(|| {
    driver::keys::DevelopmentKey::generate().map_err(|error| format!("{error:?}"))
});
static REMOTE_KEY: LazyLock<Result<RsaPrivateKey, String>> = LazyLock::new(|| {
    RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).map_err(|error| error.to_string())
});

fn remote_key() -> Result<&'static RsaPrivateKey, String> {
    REMOTE_KEY.as_ref().map_err(Clone::clone)
}

// ------------------------------------------------------- local application ----

/// Assembles the application the way `main` does, reading the settings from
/// `vars` instead of the process environment.
async fn local_app(vars: &[(&str, &str)]) -> Result<Router, String> {
    let config = driver::config::Config::from_lookup(|name: &str| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    })
    .map_err(|error| format!("{error:?}"))?;
    let key = LOCAL_KEY.as_ref().map_err(Clone::clone)?.clone();
    let handler = server::app::Handler::new(config, key).map_err(|error| format!("{error:?}"))?;
    Ok(server::router(server::app::AppModule::new(handler)))
}

async fn app_for(peer: &Peer) -> Result<Router, String> {
    local_app(&[
        (ACTOR_ENV, LOCAL_ACTOR),
        (ORIGINS_ENV, peer.origin.as_str()),
    ])
    .await
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

// ------------------------------------------------------------ remote peer ----

#[derive(Clone)]
struct ActorReply {
    status: StatusCode,
    location: Option<HeaderValue>,
    body: Vec<u8>,
}

impl ActorReply {
    fn json(document: &Value) -> Result<Self, serde_json::Error> {
        Ok(Self {
            status: StatusCode::OK,
            location: None,
            body: serde_json::to_vec(document)?,
        })
    }

    fn padded(document: &Value, len: usize) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            status: StatusCode::OK,
            location: None,
            body: padded_json(document, len)?,
        })
    }
}

impl IntoResponse for ActorReply {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            [(header::CONTENT_TYPE, "application/activity+json")],
            self.body,
        )
            .into_response();
        if let Some(location) = self.location {
            response.headers_mut().insert(header::LOCATION, location);
        }
        response
    }
}

#[derive(Clone)]
struct Delivery {
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Clone, Default)]
struct PeerLog {
    actor_fetches: Arc<AtomicUsize>,
    redirect_fetches: Arc<AtomicUsize>,
    deliveries: Arc<Mutex<Vec<Delivery>>>,
}

struct Peer {
    origin: String,
    log: PeerLog,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Peer {
    fn actor_id(&self) -> String {
        format!("{}/actor", self.origin)
    }

    fn key_id(&self) -> String {
        format!("{}/actor#main-key", self.origin)
    }

    fn authority(&self) -> &str {
        self.origin.trim_start_matches("http://")
    }

    fn actor_fetches(&self) -> usize {
        self.log.actor_fetches.load(Ordering::SeqCst)
    }

    fn redirect_fetches(&self) -> usize {
        self.log.redirect_fetches.load(Ordering::SeqCst)
    }

    fn deliveries(&self) -> Result<Vec<Delivery>, String> {
        self.log
            .deliveries
            .lock()
            .map(|deliveries| deliveries.clone())
            .map_err(|error| error.to_string())
    }
}

fn actor_document(origin: &str) -> Result<Value, Box<dyn Error>> {
    let pem = remote_key()?
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)?;
    Ok(json!({
        "@context": ["https://www.w3.org/ns/activitystreams", "https://w3id.org/security/v1"],
        "id": format!("{origin}/actor"),
        "type": "Person",
        "inbox": format!("{origin}/inbox"),
        "publicKey": {
            "id": format!("{origin}/actor#main-key"),
            "owner": format!("{origin}/actor"),
            "publicKeyPem": pem,
        },
    }))
}

/// Starts a remote actor on a loopback port. `actor` decides what `GET /actor`
/// answers; `POST /inbox` records each delivery and answers `inbox_status`.
async fn spawn_peer(
    actor: impl FnOnce(&str) -> Result<ActorReply, Box<dyn Error>>,
    inbox_status: StatusCode,
) -> Result<Peer, Box<dyn Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let reply = actor(&origin)?;
    let redirected = ActorReply::json(&actor_document(&origin)?)?;
    let log = PeerLog::default();
    let router = Router::new()
        .route(
            "/actor",
            get({
                let log = log.clone();
                move || {
                    log.actor_fetches.fetch_add(1, Ordering::SeqCst);
                    let reply = reply.clone();
                    async move { reply }
                }
            }),
        )
        .route(
            "/redirect-target",
            get({
                let log = log.clone();
                move || {
                    log.redirect_fetches.fetch_add(1, Ordering::SeqCst);
                    let reply = redirected.clone();
                    async move { reply }
                }
            }),
        )
        .route(
            "/inbox",
            post({
                let log = log.clone();
                move |headers: HeaderMap, body: Bytes| {
                    let status = match log.deliveries.lock() {
                        Ok(mut deliveries) => {
                            deliveries.push(Delivery { headers, body });
                            inbox_status
                        }
                        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
                    };
                    async move { status }
                }
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    Ok(Peer {
        origin,
        log,
        server,
    })
}

async fn peer_serving(edit: impl FnOnce(&str, &mut Value)) -> Result<Peer, Box<dyn Error>> {
    spawn_peer(
        |origin| {
            let mut document = actor_document(origin)?;
            edit(origin, &mut document);
            Ok(ActorReply::json(&document)?)
        },
        StatusCode::ACCEPTED,
    )
    .await
}

async fn valid_peer() -> Result<Peer, Box<dyn Error>> {
    peer_serving(|_, _| {}).await
}

async fn peer_with_broken_public_key() -> Result<Peer, Box<dyn Error>> {
    peer_serving(|_, document| {
        document["publicKey"]["publicKeyPem"] = json!("-----BEGIN PUBLIC KEY-----\nbroken\n");
    })
    .await
}

// ---------------------------------------------------------------- signing ----

fn follow(peer: &Peer, n: u32) -> Value {
    json!({
        "id": format!("{}/follows/{n}", peer.origin),
        "type": "Follow",
        "actor": peer.actor_id(),
        "object": LOCAL_ACTOR,
    })
}

/// Serializes `document` and pads it with JSON whitespace to exactly `len` bytes.
fn padded_json(document: &Value, len: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut body = serde_json::to_vec(document)?;
    if body.len() > len {
        return Err(format!("document is already {} bytes", body.len()).into());
    }
    body.resize(len, b' ');
    Ok(body)
}

struct Sha256Hasher;

impl ContentHasher for Sha256Hasher {
    const DIGEST_ALG: &'static str = "SHA-256";

    fn hash(content: &[u8]) -> DigestHash {
        use sha2::Digest as _;
        DigestHash::new(Sha256::digest(content).to_vec())
    }
}

struct RemoteSigner {
    key: RsaPrivateKey,
    id: String,
    algorithm: String,
}

impl SignerKey for RemoteSigner {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn algorithm(&self) -> String {
        self.algorithm.clone()
    }

    fn sign(&self, target: &[u8]) -> Vec<u8> {
        SigningKey::<Sha256>::new(self.key.clone())
            .sign(target)
            .to_vec()
    }
}

/// How the remote side signs a request to the local inbox. `for_peer` is a
/// signature the local application must accept; tests change one aspect.
struct Signing {
    key_id: String,
    algorithm: &'static str,
    covered: Vec<&'static str>,
    host: String,
    date: Option<String>,
}

impl Signing {
    fn for_peer(peer: &Peer) -> Self {
        Self {
            key_id: peer.key_id(),
            algorithm: "rsa-sha256",
            covered: REQUIRED_COMPONENTS.to_vec(),
            host: LOCAL_HOST.to_owned(),
            date: Some(httpdate::fmt_http_date(SystemTime::now())),
        }
    }

    fn dated(self, at: SystemTime) -> Self {
        Self {
            date: Some(httpdate::fmt_http_date(at)),
            ..self
        }
    }
}

#[derive(Clone)]
struct InboxRequest {
    headers: HeaderMap,
    body: Bytes,
}

async fn sign(body: Vec<u8>, signing: &Signing) -> Result<InboxRequest, Box<dyn Error>> {
    let mut builder = Request::post("/inbox")
        .header(header::HOST, &signing.host)
        .header(header::CONTENT_TYPE, "application/activity+json");
    if let Some(date) = &signing.date {
        builder = builder.header(header::DATE, date);
    }
    let request = builder
        .body(Full::new(Bytes::from(body)))?
        .digest::<Sha256Hasher>()
        .await
        .map_err(|error| format!("digest: {error:?}"))?;
    let mut params = SignatureParams::builder();
    for component in &signing.covered {
        params = if *component == "(request-target)" {
            params.add_request_target()
        } else {
            params.add_header(*component)
        };
    }
    let params = params
        .build()
        .map_err(|error| format!("params: {error:?}"))?;
    let signer = RemoteSigner {
        key: remote_key()?.clone(),
        id: signing.key_id.clone(),
        algorithm: signing.algorithm.to_owned(),
    };
    let request = request
        .sign(&signer, &params)
        .await
        .map_err(|error| format!("sign: {error:?}"))?;
    let (parts, body) = request.into_parts();
    let body = body
        .collect()
        .await
        .map_err(|error| format!("body: {error:?}"))?
        .to_bytes();
    Ok(InboxRequest {
        headers: parts.headers,
        body,
    })
}

async fn signed_follow(peer: &Peer, follow: &Value) -> Result<InboxRequest, Box<dyn Error>> {
    sign(serde_json::to_vec(follow)?, &Signing::for_peer(peer)).await
}

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
    let request = signed_follow(&peer, &follow(&peer, 1)).await?;

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
async fn f01_signed_follow_is_answered_202_after_an_accept_reaches_the_remote_inbox() -> TestResult
{
    // Given a remote actor allowed by SHUTTLEPUB_REMOTE_ORIGINS.
    let peer = valid_peer().await?;
    let app = app_for(&peer).await?;
    let follow = follow(&peer, 1);

    // When it posts a correctly signed Follow.
    let status = post_inbox(&app, signed_follow(&peer, &follow).await?).await?;

    // Then the Accept was already delivered when 202 is returned.
    assert_eq!(status, StatusCode::ACCEPTED);
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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;
    assert_eq!(status, StatusCode::ACCEPTED);

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
        post_inbox(&app, signed_follow(&peer, &first).await?).await?,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        post_inbox(&app, signed_follow(&peer, &second).await?).await?,
        StatusCode::ACCEPTED
    );

    // Then each Accept answers its own Follow under a distinct id.
    let accepts = peer
        .deliveries()?
        .iter()
        .map(|delivery| serde_json::from_slice::<Value>(&delivery.body))
        .collect::<Result<Vec<_>, _>>()?;
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
    let status = post_inbox(&app, signed_follow(&peer, &follow).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow).await?).await?;

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
    let mut request = signed_follow(&peer, &follow(&peer, 1)).await?;
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
    let valid = signed_follow(&peer, &follow(&peer, 1)).await?;
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
    let valid = signed_follow(&peer, &follow(&peer, 1)).await?;
    let fields = signature_fields(&valid.headers)?;
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
    let request = with_signature(valid, &quoted(&altered))?;

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
    let mut request = signed_follow(&peer, &follow(&peer, 1)).await?;
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
    let mut request = signed_follow(&peer, &follow(&peer, 1)).await?;
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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

    // Then it is a bad gateway.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_nothing_delivered(&peer)
}

#[tokio::test]
async fn f12_accept_rejected_by_the_remote_inbox_is_answered_502() -> TestResult {
    // Given a remote inbox that answers the Accept with an error status.
    let peer = spawn_peer(
        |origin| Ok(ActorReply::json(&actor_document(origin)?)?),
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await?;
    let app = app_for(&peer).await?;

    // When a correctly signed Follow is posted.
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

    // Then the delivery was attempted and the failure is a bad gateway.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(peer.deliveries()?.len(), 1);
    Ok(())
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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
    let status = post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?;

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
        post_inbox(&app, signed_follow(&peer, &follow).await?).await?,
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
        post_inbox(&app, signed_follow(&peer, &follow).await?).await?,
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
        post_inbox(&app, signed_follow(&peer, &follow(&peer, 1)).await?).await?,
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
    let mut request = signed_follow(&peer, &follow(&peer, 1)).await?;
    request.body = Bytes::from(serde_json::to_vec(&follow(&peer, 2))?);

    assert_eq!(post_inbox(&app, request).await?, StatusCode::BAD_GATEWAY);
    Ok(())
}
