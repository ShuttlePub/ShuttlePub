//! Federation adapters.
mod federation;
mod keys;
mod signature;
mod wire;

pub use federation::Federation;
pub use keys::DevelopmentKey;
pub use signature::{sign_request, verify_request};
pub use wire::{FederationError, Follow, RemoteActor};
