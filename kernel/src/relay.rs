//! The federation inbox and delivery: Follow and Accept, remote actors, the
//! followers of the local actor, and the ports that fetch actors, verify
//! signatures, deliver activities and record and list followers. Each port
//! trait `Foo` is paired with `DependOnFoo`, which a composition root
//! implements to hand out its adapter.
mod accept;
mod delivery;
mod follow;
mod follower;
mod follower_event;
mod followers;
mod remote_actor;
mod remote_actor_resolver;
mod signature;

pub use accept::Accept;
pub use delivery::{ActivityDelivery, DependOnActivityDelivery};
pub use follow::Follow;
pub use follower::{AcceptFollow, Follower};
pub use follower_event::FollowerEvent;
pub use followers::{
    DependOnFollowerReader, DependOnFollowerRecorder, FollowerEntry, FollowerReader,
    FollowerRecorder,
};
pub use remote_actor::RemoteActor;
pub use remote_actor_resolver::{DependOnRemoteActorResolver, RemoteActorResolver};
pub use signature::{DependOnHttpSignatureVerifier, HttpSignatureVerifier};
