//! The local actor's identity: its in-memory signing key and the actor itself.
mod key;
mod local;

pub use key::DevelopmentKey;
pub use local::LocalIdentity;
