//! Identities: the ids actors and activities are known by, public keys, and
//! the actor this server speaks as with the port that provides it.
mod id;
mod key;
mod local_actor;
mod local_actor_provider;

pub use id::{ActivityId, ActorId};
pub use key::PublicKey;
pub use local_actor::LocalActor;
pub use local_actor_provider::{DependOnLocalActorProvider, LocalActorProvider};
