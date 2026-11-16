//! Adapters implementing the kernel ports, grouped like the kernel: `relay`
//! (the remote HTTP client for actor lookup and Accept delivery, HTTP
//! signatures, and the follower journal, read model and projection),
//! `identity` (the development signing key and the local actor) and
//! `persistence` (PostgreSQL, migrations and the nitinol event sourcing
//! system); plus loading the settings from the environment.
pub mod config;
pub mod error;
pub mod identity;
pub mod persistence;
pub mod relay;

pub use error::DriverError;
