use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime},
};

use base64::Engine;
use bytes::Bytes;
use http::{HeaderMap, Request};
use http_body_util::Full;
use http_msgsign_draft::{
    digest::{ContentHasher, Digest, DigestHash, body::Body},
    sign::{RequestSign, SignatureParams, headers::SignatureInput},
};

use crate::{
    DevelopmentKey, FederationError, RemoteActor,
    keys::{SigningIdentity, VerificationIdentity},
};

pub struct Sha256Hasher;

impl ContentHasher for Sha256Hasher {
    const DIGEST_ALG: &'static str = "SHA-256";
    fn hash(content: &[u8]) -> DigestHash {
        use sha2::Digest;
        DigestHash::new(sha2::Sha256::digest(content).to_vec())
    }
}

// Guard the upstream draft parser's unchecked Base64/header conversions and
// require body, destination and freshness to be cryptographically bound.
pub(crate) fn signature_input(headers: &HeaderMap) -> Result<SignatureInput, FederationError> {
    let raw = headers
        .get("signature")
        .ok_or(FederationError::Signature)?
        .to_str()
        .map_err(|_| FederationError::Signature)?;
    let mut fields = BTreeMap::new();
    for pair in raw.split(',') {
        let (key, value) = pair
            .trim()
            .split_once('=')
            .ok_or(FederationError::Signature)?;
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .ok_or(FederationError::Signature)?;
        if fields.insert(key, value).is_some() {
            return Err(FederationError::Signature);
        }
    }
    let signed = fields.get("headers").ok_or(FederationError::Signature)?;
    for required in ["(request-target)", "host", "date", "digest", "content-type"] {
        if !signed.split(' ').any(|header| header == required) {
            return Err(FederationError::Signature);
        }
    }
    for header in signed.split(' ') {
        if header != "(request-target)" && http::HeaderName::from_bytes(header.as_bytes()).is_err()
        {
            return Err(FederationError::Signature);
        }
    }
    base64::engine::general_purpose::STANDARD
        .decode(fields.get("signature").ok_or(FederationError::Signature)?)
        .map_err(|_| FederationError::Signature)?;
    let normalized = fields
        .iter()
        .map(|(key, value)| format!("{key}=\"{value}\""))
        .collect::<Vec<_>>()
        .join(",");
    let mut safe_headers = HeaderMap::new();
    safe_headers.insert(
        "signature",
        normalized.parse().map_err(|_| FederationError::Signature)?,
    );
    SignatureInput::from_header(&safe_headers).map_err(|_| FederationError::Signature)
}

pub async fn sign_request(
    key: &DevelopmentKey,
    key_id: &str,
    destination: &str,
    body: Vec<u8>,
) -> Result<Request<Body>, FederationError> {
    let uri: http::Uri = destination.parse().map_err(|_| FederationError::Identity)?;
    let host = uri
        .authority()
        .ok_or(FederationError::Identity)?
        .to_string();
    let request = Request::post(uri)
        .header("host", host)
        .header("date", httpdate::fmt_http_date(SystemTime::now()))
        .header("content-type", "application/activity+json")
        .body(Full::new(Bytes::from(body)))?;
    let params = SignatureParams::builder()
        .add_request_target()
        .add_header("host")
        .add_header("date")
        .add_header("digest")
        .add_header("content-type")
        .build()
        .map_err(|_| FederationError::Signature)?;
    let request = request
        .digest::<Sha256Hasher>()
        .await
        .map_err(|_| FederationError::Signature)?;
    request
        .sign(
            &SigningIdentity {
                key: key.clone(),
                id: key_id.into(),
            },
            &params,
        )
        .await
        .map_err(|_| FederationError::Signature)
}

pub async fn verify_request(
    request: Request<Full<Bytes>>,
    actor: &RemoteActor,
) -> Result<Request<Body>, FederationError> {
    let input = signature_input(request.headers())?;
    if input.key_id() != actor.public_key.id
        || actor.public_key.owner != actor.id
        || input.algorithm() != "rsa-sha256"
    {
        return Err(FederationError::Identity);
    }
    let date = request
        .headers()
        .get("date")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| httpdate::parse_http_date(v).ok())
        .ok_or(FederationError::Signature)?;
    let now = SystemTime::now();
    let age = now
        .duration_since(date)
        .or_else(|_| date.duration_since(now))
        .map_err(|_| FederationError::Signature)?;
    if age > Duration::from_secs(300) {
        return Err(FederationError::Signature);
    }
    let key = VerificationIdentity::new(&actor.public_key.id, &actor.public_key.public_key_pem)?;
    let request = request
        .verify_digest::<Sha256Hasher>()
        .await
        .map_err(|_| FederationError::Signature)?;
    input
        .verify_request(&request, &key)
        .map_err(|_| FederationError::Signature)?;
    Ok(request)
}
