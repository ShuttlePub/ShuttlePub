//! The domain: the aggregates of the producers that federate with this
//! service and of the Notes they send, their commands, events and storage
//! codecs, the values they are made of, and the ports that are not external
//! services (the local actor, HTTP signature verification).
//!
//! Modules follow the domain's groupings: `community` (this service itself),
//! `producer` (a service that asked to federate), `intake` (checking what
//! arrives), `note` (a Note that arrived). `id` holds the identifiers more than
//! one of them uses.
pub mod community;
pub mod error;
pub mod id;
pub mod intake;
pub mod note;
pub mod producer;

pub use error::KernelError;
