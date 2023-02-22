use kernel::KernelError;

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("failed execute transation. `sqlx`: {0}")]
    SqlX(#[from] sqlx::Error)
}

impl From<DriverError> for KernelError {
    fn from(driver: DriverError) -> Self {
        match driver {
            DriverError::SqlX(e) => KernelError::Driver(anyhow::Error::new(e))
        }
    }
}