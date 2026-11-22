//! How a remote service signs a request to the local inbox.

use std::{error::Error, time::SystemTime};

use axum::{
    body::Bytes,
    http::{HeaderMap, Request, header},
};
use http_body_util::{BodyExt, Full};
use http_msgsign_draft::{
    digest::{ContentHasher, Digest, DigestHash},
    sign::{RequestSign, SignatureParams, SignerKey},
};
use rsa::{
    RsaPrivateKey,
    pkcs1v15::SigningKey,
    signature::{SignatureEncoding, Signer},
};
use serde_json::Value;
use sha2::Sha256;

use super::{
    activity::LOCAL_HOST,
    peer::{Peer, remote_key},
};

pub const REQUIRED_COMPONENTS: [&str; 5] =
    ["(request-target)", "host", "date", "digest", "content-type"];

pub struct Sha256Hasher;

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
pub struct Signing {
    pub key_id: String,
    pub algorithm: &'static str,
    pub covered: Vec<&'static str>,
    pub host: String,
    pub date: Option<String>,
}

impl Signing {
    pub fn for_peer(peer: &Peer) -> Self {
        Self {
            key_id: peer.key_id(),
            algorithm: "rsa-sha256",
            covered: REQUIRED_COMPONENTS.to_vec(),
            host: LOCAL_HOST.to_owned(),
            date: Some(httpdate::fmt_http_date(SystemTime::now())),
        }
    }

    /// Signed by the peer's second actor rather than by its `/actor`.
    pub fn for_member(peer: &Peer) -> Self {
        Self {
            key_id: peer.member_key_id(),
            ..Self::for_peer(peer)
        }
    }

    pub fn dated(self, at: SystemTime) -> Self {
        Self {
            date: Some(httpdate::fmt_http_date(at)),
            ..self
        }
    }
}

#[derive(Clone)]
pub struct InboxRequest {
    pub headers: HeaderMap,
    pub body: Bytes,
}

pub async fn sign(body: Vec<u8>, signing: &Signing) -> Result<InboxRequest, Box<dyn Error>> {
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

/// `activity`, correctly signed by the peer's `/actor`.
pub async fn signed(peer: &Peer, activity: &Value) -> Result<InboxRequest, Box<dyn Error>> {
    sign(serde_json::to_vec(activity)?, &Signing::for_peer(peer)).await
}
