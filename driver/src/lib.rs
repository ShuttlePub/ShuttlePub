//! Adapters implementing the kernel ports: the remote HTTP client (actor
//! lookup and Accept delivery), HTTP signatures, the development signing key
//! and the local actor; plus loading the settings from the environment.
pub mod config;
pub mod error;
pub mod keys;
pub mod local;
pub mod remote;
pub mod signature;

pub use error::DriverError;
