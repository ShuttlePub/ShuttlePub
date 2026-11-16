//! The federation inbox: answering a Follow of the local actor.
mod accept_follow;
mod transfer;

pub use accept_follow::{AcceptFollowService, DependOnAcceptFollowService};
pub use transfer::{IdentifiedFollow, InboundFollow};
