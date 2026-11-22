//! A remote ActivityPub service on a loopback port. The application resolves
//! actors and delivers Accepts over HTTP, so the remote side is a real server.

use std::{
    error::Error,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rsa::{
    RsaPrivateKey,
    pkcs8::{EncodePublicKey, LineEnding},
};
use serde_json::{Value, json};

use super::wait::{REACTION_DEADLINE, eventually};

static REMOTE_KEY: LazyLock<Result<RsaPrivateKey, String>> = LazyLock::new(|| {
    RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).map_err(|error| error.to_string())
});

/// The key every actor of every remote service signs with.
pub fn remote_key() -> Result<&'static RsaPrivateKey, String> {
    REMOTE_KEY.as_ref().map_err(Clone::clone)
}

#[derive(Clone)]
pub struct ActorReply {
    pub status: StatusCode,
    pub location: Option<HeaderValue>,
    pub body: Vec<u8>,
}

impl ActorReply {
    pub fn json(document: &Value) -> Result<Self, serde_json::Error> {
        Ok(Self {
            status: StatusCode::OK,
            location: None,
            body: serde_json::to_vec(document)?,
        })
    }

    pub fn padded(document: &Value, len: usize) -> Result<Self, Box<dyn Error>> {
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

/// How the remote `POST /inbox` answers a delivery it has recorded.
#[derive(Clone, Copy)]
pub enum InboxReply {
    Status(StatusCode),
    /// Never answers, like a service that accepted the connection and hung.
    Hang,
}

#[derive(Clone)]
pub struct Delivery {
    pub headers: HeaderMap,
    pub body: Bytes,
}

#[derive(Clone, Default)]
struct PeerLog {
    actor_fetches: Arc<AtomicUsize>,
    redirect_fetches: Arc<AtomicUsize>,
    deliveries: Arc<Mutex<Vec<Delivery>>>,
}

pub struct Peer {
    pub origin: String,
    log: PeerLog,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Peer {
    pub fn actor_id(&self) -> String {
        format!("{}/actor", self.origin)
    }

    pub fn key_id(&self) -> String {
        format!("{}/actor#main-key", self.origin)
    }

    /// A second actor of the same service, so on the same authority.
    pub fn member_id(&self) -> String {
        format!("{}{MEMBER_PATH}", self.origin)
    }

    pub fn member_key_id(&self) -> String {
        format!("{}{MEMBER_PATH}#main-key", self.origin)
    }

    pub fn authority(&self) -> &str {
        self.origin.trim_start_matches("http://")
    }

    pub fn actor_fetches(&self) -> usize {
        self.log.actor_fetches.load(Ordering::SeqCst)
    }

    pub fn redirect_fetches(&self) -> usize {
        self.log.redirect_fetches.load(Ordering::SeqCst)
    }

    pub fn deliveries(&self) -> Result<Vec<Delivery>, String> {
        self.log
            .deliveries
            .lock()
            .map(|deliveries| deliveries.clone())
            .map_err(|error| error.to_string())
    }

    /// Waits until at least `count` deliveries have reached this inbox.
    pub async fn wait_for_deliveries(&self, count: usize) -> Result<(), Box<dyn Error>> {
        eventually(
            &format!("{count} deliveries to {}", self.origin),
            REACTION_DEADLINE,
            async || Ok(self.deliveries()?.len() >= count),
        )
        .await
    }

    /// The bodies of the Accepts delivered so far, in arrival order.
    pub fn accepts(&self) -> Result<Vec<Value>, Box<dyn Error>> {
        Ok(self
            .deliveries()?
            .iter()
            .map(|delivery| serde_json::from_slice::<Value>(&delivery.body))
            .collect::<Result<Vec<_>, _>>()?)
    }
}

const MEMBER_PATH: &str = "/users/alice";

pub fn actor_document(origin: &str) -> Result<Value, Box<dyn Error>> {
    actor_document_at(origin, "/actor")
}

fn actor_document_at(origin: &str, path: &str) -> Result<Value, Box<dyn Error>> {
    let pem = remote_key()?
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)?;
    Ok(json!({
        "@context": ["https://www.w3.org/ns/activitystreams", "https://w3id.org/security/v1"],
        "id": format!("{origin}{path}"),
        "type": "Person",
        "inbox": format!("{origin}/inbox"),
        "publicKey": {
            "id": format!("{origin}{path}#main-key"),
            "owner": format!("{origin}{path}"),
            "publicKeyPem": pem,
        },
    }))
}

/// Starts a remote service on a loopback port. `actor` decides what
/// `GET /actor` answers; `POST /inbox` records each delivery and answers
/// `inbox_status`.
pub async fn spawn_peer(
    actor: impl FnOnce(&str) -> Result<ActorReply, Box<dyn Error>>,
    inbox_status: StatusCode,
) -> Result<Peer, Box<dyn Error>> {
    spawn_peer_with_inbox(actor, InboxReply::Status(inbox_status)).await
}

pub async fn spawn_peer_with_inbox(
    actor: impl FnOnce(&str) -> Result<ActorReply, Box<dyn Error>>,
    inbox: InboxReply,
) -> Result<Peer, Box<dyn Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let reply = actor(&origin)?;
    let redirected = ActorReply::json(&actor_document(&origin)?)?;
    let member = ActorReply::json(&actor_document_at(&origin, MEMBER_PATH)?)?;
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
            MEMBER_PATH,
            get(move || {
                let reply = member.clone();
                async move { reply }
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
                    let recorded = log
                        .deliveries
                        .lock()
                        .map(|mut deliveries| deliveries.push(Delivery { headers, body }))
                        .is_ok();
                    async move {
                        match (recorded, inbox) {
                            (false, _) => StatusCode::INTERNAL_SERVER_ERROR,
                            (true, InboxReply::Status(status)) => status,
                            (true, InboxReply::Hang) => std::future::pending().await,
                        }
                    }
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

pub async fn peer_serving(edit: impl FnOnce(&str, &mut Value)) -> Result<Peer, Box<dyn Error>> {
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

pub async fn valid_peer() -> Result<Peer, Box<dyn Error>> {
    peer_serving(|_, _| {}).await
}

/// A valid actor whose inbox records each Accept and never answers it.
pub async fn unresponsive_peer() -> Result<Peer, Box<dyn Error>> {
    spawn_peer_with_inbox(
        |origin| Ok(ActorReply::json(&actor_document(origin)?)?),
        InboxReply::Hang,
    )
    .await
}

pub async fn peer_with_broken_public_key() -> Result<Peer, Box<dyn Error>> {
    peer_serving(|_, document| {
        document["publicKey"]["publicKeyPem"] = json!("-----BEGIN PUBLIC KEY-----\nbroken\n");
    })
    .await
}

/// Serializes `document` and pads it with JSON whitespace to exactly `len` bytes.
pub fn padded_json(document: &Value, len: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut body = serde_json::to_vec(document)?;
    if body.len() > len {
        return Err(format!("document is already {} bytes", body.len()).into());
    }
    body.resize(len, b' ');
    Ok(body)
}
