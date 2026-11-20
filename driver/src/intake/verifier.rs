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
    digest::Digest,
    errors::VerificationError,
    sign::{VerifierKey, headers::SignatureInput},
};
use kernel::{KernelError, community::PublicKey, intake::HttpSignatureVerifier};
use rsa::{
    RsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
    signature::Verifier,
};
use sha2::Sha256;

use crate::signature::{ALGORITHM, REQUEST_TARGET, REQUIRED_COMPONENTS, Sha256Hasher};

const MAX_CLOCK_SKEW: Duration = Duration::from_secs(300);

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
