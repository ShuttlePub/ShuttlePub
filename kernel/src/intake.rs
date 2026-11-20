//! Checking what arrives at the inbox: the sender as fetched from its server
//! and the verification of the request's HTTP signature.
mod remote_actor;
mod signature;

pub use remote_actor::RemoteActor;
pub use signature::{DependOnHttpSignatureVerifier, HttpSignatureVerifier};
