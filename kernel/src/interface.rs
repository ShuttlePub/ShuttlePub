//! Ports to external systems. Each port trait `Foo` is paired with
//! `DependOnFoo`, which a composition root implements to hand out its adapter.
mod delivery;
mod local_actor;
mod remote_actor;
mod signature;

pub use delivery::{ActivityDelivery, DependOnActivityDelivery};
pub use local_actor::{DependOnLocalActorProvider, LocalActorProvider};
pub use remote_actor::{DependOnRemoteActorResolver, RemoteActorResolver};
pub use signature::{DependOnHttpSignatureVerifier, HttpSignatureVerifier};
