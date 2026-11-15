//! draft-cavage HTTP signatures as used by ActivityPub servers: `rsa-sha256`
//! over `(request-target) host date digest content-type`, with a SHA-256
//! `Digest` of the body.

use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime},
};

use base64::Engine;
use bytes::Bytes;
use error_stack::{Report, ResultExt};
use http::{HeaderMap, HeaderName, Request, header};
use http_body_util::Full;
use http_msgsign_draft::{
    digest::{ContentHasher, Digest, DigestHash, body::Body},
    errors::VerificationError,
    sign::{RequestSign, SignatureParams, SignerKey, VerifierKey, headers::SignatureInput},
};
use kernel::{KernelError, entities::PublicKey, interface::HttpSignatureVerifier};
use rsa::{
    RsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
    signature::Verifier,
};
use sha2::Sha256;

use crate::keys::DevelopmentKey;

const ALGORITHM: &str = "rsa-sha256";
const REQUEST_TARGET: &str = "(request-target)";
const REQUIRED_COMPONENTS: [&str; 5] = [REQUEST_TARGET, "host", "date", "digest", "content-type"];
const MAX_CLOCK_SKEW: Duration = Duration::from_secs(300);
const ACTIVITY_JSON: &str = "application/activity+json";

/// Verifies signatures of requests other servers send to the local inbox.
pub struct SignatureVerifier;

impl HttpSignatureVerifier for SignatureVerifier {
    fn inspect(&self, request: &Request<Bytes>) -> Result<String, Report<KernelError>> {
        Ok(signature_input(request.headers())?.key_id().to_owned())
    }

    async fn verify(
        &self,
        request: &Request<Bytes>,
        key: &PublicKey,
    ) -> Result<(), Report<KernelError>> {
        let input = signature_input(request.headers())?;
        if input.algorithm() != ALGORITHM {
            return Err(Report::new(KernelError::InvalidSignature)
                .attach(format!("unsupported algorithm {}", input.algorithm())));
        }
        check_freshness(request.headers())?;
        let key = RemoteKey::new(key)?;
        let request = copy_request(request)
            .verify_digest::<Sha256Hasher>()
            .await
            .change_context(KernelError::InvalidSignature)?;
        input
            .verify_request(&request, &key)
            .change_context(KernelError::InvalidSignature)
    }
}

/// Validates the `Signature` header before handing it to http-msgsign-draft,
/// whose parser tolerates duplicated items and unquoted values and panics on
/// invalid Base64 or header names.
fn signature_input(headers: &HeaderMap) -> Result<SignatureInput, Report<KernelError>> {
    let invalid = || Report::new(KernelError::InvalidSignature);
    let raw = headers
        .get("signature")
        .ok_or_else(|| invalid().attach("no Signature header"))?
        .to_str()
        .change_context(KernelError::InvalidSignature)?;
    let mut items = BTreeMap::new();
    for item in raw.split(',') {
        let (name, value) = item
            .trim()
            .split_once('=')
            .ok_or_else(|| invalid().attach("Signature item without `=`"))?;
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or_else(|| invalid().attach("unquoted Signature value"))?;
        if items.insert(name, value).is_some() {
            return Err(invalid().attach(format!("duplicated Signature item {name}")));
        }
    }
    let covered = items
        .get("headers")
        .ok_or_else(|| invalid().attach("Signature without headers"))?;
    for required in REQUIRED_COMPONENTS {
        if !covered.split(' ').any(|component| component == required) {
            return Err(invalid().attach(format!("{required} is not signed")));
        }
    }
    for component in covered.split(' ') {
        if component != REQUEST_TARGET && HeaderName::from_bytes(component.as_bytes()).is_err() {
            return Err(invalid().attach("invalid header name in Signature headers"));
        }
    }
    base64::engine::general_purpose::STANDARD
        .decode(
            items
                .get("signature")
                .ok_or_else(|| invalid().attach("Signature without signature"))?,
        )
        .change_context(KernelError::InvalidSignature)?;
    let normalized = items
        .iter()
        .map(|(name, value)| format!("{name}=\"{value}\""))
        .collect::<Vec<_>>()
        .join(",");
    let mut checked = HeaderMap::new();
    checked.insert(
        "signature",
        normalized
            .parse()
            .change_context(KernelError::InvalidSignature)?,
    );
    SignatureInput::from_header(&checked).change_context(KernelError::InvalidSignature)
}

fn check_freshness(headers: &HeaderMap) -> Result<(), Report<KernelError>> {
    let date = headers
        .get(header::DATE)
        .ok_or_else(|| Report::new(KernelError::InvalidSignature).attach("no Date header"))?
        .to_str()
        .change_context(KernelError::InvalidSignature)?;
    let date = httpdate::parse_http_date(date).change_context(KernelError::InvalidSignature)?;
    let now = SystemTime::now();
    let skew = now
        .duration_since(date)
        .or_else(|_| date.duration_since(now))
        .change_context(KernelError::InvalidSignature)?;
    if skew > MAX_CLOCK_SKEW {
        return Err(Report::new(KernelError::InvalidSignature)
            .attach(format!("Date is {}s away from now", skew.as_secs())));
    }
    Ok(())
}

fn copy_request(request: &Request<Bytes>) -> Request<Full<Bytes>> {
    let mut copy = Request::new(Full::new(request.body().clone()));
    *copy.method_mut() = request.method().clone();
    *copy.uri_mut() = request.uri().clone();
    *copy.version_mut() = request.version();
    *copy.headers_mut() = request.headers().clone();
    copy
}

struct Sha256Hasher;

impl ContentHasher for Sha256Hasher {
    const DIGEST_ALG: &'static str = "SHA-256";

    fn hash(content: &[u8]) -> DigestHash {
        use sha2::Digest as _;
        DigestHash::new(Sha256::digest(content).to_vec())
    }
}

struct RemoteKey {
    id: String,
    key: VerifyingKey<Sha256>,
}

impl RemoteKey {
    /// An unreadable key is the remote server's fault, not the request's.
    fn new(key: &PublicKey) -> Result<Self, Report<KernelError>> {
        let public = RsaPublicKey::from_public_key_pem(key.pem())
            .change_context(KernelError::RemoteFailure)
            .attach(format!("public key {}", key.id()))?;
        Ok(Self {
            id: key.id().to_owned(),
            key: VerifyingKey::new(public),
        })
    }
}

impl VerifierKey for RemoteKey {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn algorithm(&self) -> String {
        ALGORITHM.to_owned()
    }

    fn verify(&self, target: &[u8], signature: &[u8]) -> Result<(), VerificationError> {
        let signature = Signature::try_from(signature)
            .map_err(|error| VerificationError::Crypto(Box::new(error)))?;
        self.key
            .verify(target, &signature)
            .map_err(|error| VerificationError::Crypto(Box::new(error)))
    }
}

/// Signs requests the local actor sends to other servers.
pub(crate) struct RequestSigner {
    key: DevelopmentKey,
    key_id: String,
}

impl RequestSigner {
    pub(crate) fn new(key: DevelopmentKey, key_id: String) -> Self {
        Self { key, key_id }
    }

    /// Builds a signed `POST` of an activity to `destination`.
    pub(crate) async fn sign_post(
        &self,
        destination: &str,
        body: Vec<u8>,
    ) -> Result<Request<Body>, Report<KernelError>> {
        let uri: http::Uri = destination
            .parse()
            .change_context(KernelError::UntrustedIdentity)?;
        let host = uri
            .authority()
            .ok_or_else(|| {
                Report::new(KernelError::UntrustedIdentity).attach("destination without authority")
            })?
            .to_string();
        let request = Request::post(uri)
            .header(header::HOST, host)
            .header(header::DATE, httpdate::fmt_http_date(SystemTime::now()))
            .header(header::CONTENT_TYPE, ACTIVITY_JSON)
            .body(Full::new(Bytes::from(body)))
            .change_context(KernelError::Malformed)?;
        let params = REQUIRED_COMPONENTS
            .into_iter()
            .fold(SignatureParams::builder(), |params, component| {
                if component == REQUEST_TARGET {
                    params.add_request_target()
                } else {
                    params.add_header(component)
                }
            })
            .build()
            .change_context(KernelError::InvalidSignature)?;
        request
            .digest::<Sha256Hasher>()
            .await
            .change_context(KernelError::InvalidSignature)?
            .sign(self, &params)
            .await
            .change_context(KernelError::InvalidSignature)
    }
}

impl SignerKey for RequestSigner {
    fn id(&self) -> String {
        self.key_id.clone()
    }

    fn algorithm(&self) -> String {
        ALGORITHM.to_owned()
    }

    fn sign(&self, target: &[u8]) -> Vec<u8> {
        self.key.sign(target)
    }
}
