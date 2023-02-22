use kernel::KernelError;

#[derive(Debug, thiserror::Error)]
pub enum ApplicationError {
    #[error("cannot find `{id}:{entity}` in the following {method}.")]
    NotFound {
        method: &'static str,
        entity: &'static str,
        id: String
    },
    #[error("this value illegal. {0}")]
    Convert(String),
    #[error(transparent)]
    External(anyhow::Error)
}

impl From<KernelError> for ApplicationError {
    fn from(kernel: KernelError) -> Self {
        match kernel {
            KernelError::NotFound { method, entity, id } 
              => ApplicationError::NotFound { method, entity, id },
            KernelError::Convert(msg) => ApplicationError::Convert(msg),
            KernelError::External(err) => ApplicationError::External(err),
            KernelError::Driver(err) => ApplicationError::External(err)
        }
    }
}