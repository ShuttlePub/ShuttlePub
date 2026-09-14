use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKey {
    pub id: String,
    pub owner: String,
    pub public_key_pem: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteActor {
    pub id: String,
    pub inbox: String,
    pub public_key: PublicKey,
}

impl RemoteActor {
    pub fn new(id: &str, inbox: &str, pem: String) -> Self {
        Self {
            id: id.into(),
            inbox: inbox.into(),
            public_key: PublicKey {
                id: format!("{id}#main-key"),
                owner: id.into(),
                public_key_pem: pem,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FollowType {
    Follow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Follow {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: FollowType,
    pub actor: String,
    pub object: String,
}

impl Follow {
    pub fn new(id: &str, actor: &str, object: &str) -> Self {
        Self {
            id: id.into(),
            kind: FollowType::Follow,
            actor: actor.into(),
            object: object.into(),
        }
    }
}

#[derive(Serialize)]
pub struct Accept<'a> {
    #[serde(rename = "@context")]
    pub context: &'static str,
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub actor: &'a str,
    pub object: &'a Follow,
}

#[derive(Debug, thiserror::Error)]
pub enum FederationError {
    #[error("invalid or unsupported signature")]
    Signature,
    #[error("untrusted actor identity or destination")]
    Identity,
    #[error("remote origin is not allowed")]
    Origin,
    #[error("remote response exceeds size limit")]
    ResponseSize,
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Request(#[from] http::Error),
    #[error(transparent)]
    Rsa(#[from] rsa::errors::Error),
    #[error(transparent)]
    Pem(#[from] rsa::pkcs8::spki::Error),
}
