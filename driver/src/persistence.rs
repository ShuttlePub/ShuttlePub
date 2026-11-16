//! What records and runs: the PostgreSQL connection and its migrations, the
//! nitinol event sourcing system, and the handle of a running projection.
mod database;
mod event_sourcing;
mod projection;
mod settings;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use database::Database;
pub use event_sourcing::EventSourcing;
pub(crate) use projection::PoolTransactions;
pub use projection::Projection;
pub use settings::DatabaseSettings;
