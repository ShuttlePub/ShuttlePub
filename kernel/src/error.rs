/// Failure classes shared by every port. Adapters wrap their own errors in one
/// of these so that callers can decide what to do without knowing the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KernelError {
    #[error("the request signature is missing, malformed or does not verify")]
    InvalidSignature,
    #[error("the actor, key or target does not match the expected identity")]
    UntrustedIdentity,
    #[error("the remote origin is not allowed")]
    OriginNotAllowed,
    #[error("the input is malformed")]
    Malformed,
    #[error("the remote server failed or answered unusably")]
    RemoteFailure,
    #[error("an internal operation failed")]
    Internal,
}
