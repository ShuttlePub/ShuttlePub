//! The activities a remote service sends to the local inbox.

use serde_json::{Value, json};

use super::peer::Peer;

pub const LOCAL_ACTOR: &str = "https://local.example/actor";
pub const LOCAL_HOST: &str = "local.example";

/// The three spellings of the public collection a receiver must understand.
pub const PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";
pub const PUBLIC_COMPACT: &str = "as:Public";
pub const PUBLIC_BARE: &str = "Public";

pub fn follow(peer: &Peer, n: u32) -> Value {
    json!({
        "id": format!("{}/follows/{n}", peer.origin),
        "type": "Follow",
        "actor": peer.actor_id(),
        "object": LOCAL_ACTOR,
    })
}

/// A public Note written by `author` on the peer's origin.
pub fn note_by(peer: &Peer, author: &str, n: u32) -> Value {
    json!({
        "id": format!("{}/notes/{n}", peer.origin),
        "type": "Note",
        "attributedTo": author,
        "content": format!("note {n}"),
        "to": [PUBLIC],
        "cc": [format!("{author}/followers")],
    })
}

/// A public Note written by the peer's `/actor`.
pub fn note(peer: &Peer, n: u32) -> Value {
    note_by(peer, &peer.actor_id(), n)
}

/// `note` with its audience replaced by `to` and `cc`.
pub fn addressed(mut note: Value, to: Value, cc: Value) -> Value {
    note["to"] = to;
    note["cc"] = cc;
    note
}

/// A Create by `actor` that embeds `note`. The Create carries the Note's
/// audience, as ActivityPub servers send it.
pub fn create_by(peer: &Peer, actor: &str, n: u32, note: &Value) -> Value {
    json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/creates/{n}", peer.origin),
        "type": "Create",
        "actor": actor,
        "object": note,
        "to": note["to"],
        "cc": note["cc"],
    })
}

/// A Create by the peer's `/actor` that embeds `note`.
pub fn create(peer: &Peer, n: u32, note: &Value) -> Value {
    create_by(peer, &peer.actor_id(), n, note)
}
