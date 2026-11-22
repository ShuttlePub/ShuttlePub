use crate::id::{ActivityId, ActorId};

/// Accepts the federation `actor` asked for with the Follow `follow`; the
/// Accept is to be delivered to `inbox`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptFederation {
    pub actor: ActorId,
    pub inbox: String,
    pub follow: ActivityId,
}
