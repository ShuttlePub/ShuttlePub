use error_stack::Report;
use kernel::KernelError;

/// Failure classes of the use cases. Port failures keep the class the adapter
/// gave them; the use cases add their own identity and input checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApplicationError {
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

impl ApplicationError {
    /// Lifts a port failure into the use-case error, keeping its class.
    pub fn from_kernel(report: Report<KernelError>) -> Report<Self> {
        let class = match report.current_context() {
            KernelError::InvalidSignature => Self::InvalidSignature,
            KernelError::UntrustedIdentity => Self::UntrustedIdentity,
            KernelError::OriginNotAllowed => Self::OriginNotAllowed,
            KernelError::Malformed => Self::Malformed,
            KernelError::RemoteFailure => Self::RemoteFailure,
            KernelError::Internal => Self::Internal,
        };
        report.change_context(class)
    }
}
