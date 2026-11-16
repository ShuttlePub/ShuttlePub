/// Failures while assembling the adapters or stopping them. Failures while
/// serving a request are reported through the kernel ports as `KernelError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DriverError {
    #[error("a setting is invalid")]
    InvalidSetting,
    #[error("the signing key could not be generated or encoded")]
    Key,
    #[error("the HTTP client could not be built")]
    HttpClient,
    #[error("the database could not be reached, migrated or opened")]
    Database,
    #[error("the projection process could not be reached")]
    Projection,
}
