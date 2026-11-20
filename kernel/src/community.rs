//! This service itself: the actor it speaks as and the key it publishes.
mod key;
mod local_actor;
mod provider;

pub use key::PublicKey;
pub use local_actor::LocalActor;
pub use provider::{DependOnLocalActorProvider, LocalActorProvider};
