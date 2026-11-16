//! The federation adapters: the HTTP client for other servers, HTTP
//! signatures, and the followers of the local actor.
mod followers;
mod remote;
mod signature;

pub use followers::{FollowerJournal, FollowerReadModel, start_follower_projection};
pub use remote::RemoteClient;
pub use signature::SignatureVerifier;
