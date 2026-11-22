//! Adapters: the remote HTTP client implementing the `port` traits (actor
//! lookup in `intake`, Accept delivery in `delivery`), HTTP signature
//! verification and signing, the development signing key and the local actor
//! (`community`); plus loading the settings from the environment.
pub mod community;
pub mod config;
pub mod delivery;
pub mod error;
pub mod intake;
pub mod keys;
pub mod remote;
mod signature;

pub use error::DriverError;
