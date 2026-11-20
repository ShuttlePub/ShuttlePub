use bytes::Bytes;
use error_stack::Report;
use http::Request;

use crate::{KernelError, community::PublicKey};

/// Verifies the HTTP signature of a received request in two steps, so that
/// the key can be looked up between them.
pub trait HttpSignatureVerifier: Send + Sync + 'static {
    /// Checks the `Signature` header's syntax and that it covers every
    /// required component, and returns the `keyId` it claims.
    fn inspect(&self, request: &Request<Bytes>) -> Result<String, Report<KernelError>>;

    /// Checks the algorithm, freshness, body digest and signature against `key`.
    fn verify(
        &self,
        request: &Request<Bytes>,
        key: &PublicKey,
    ) -> impl Future<Output = Result<(), Report<KernelError>>> + Send;
}

pub trait DependOnHttpSignatureVerifier: Send + Sync + 'static {
    type HttpSignatureVerifier: HttpSignatureVerifier;
    fn http_signature_verifier(&self) -> &Self::HttpSignatureVerifier;
}
