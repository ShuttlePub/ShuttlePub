/// Public key an actor publishes for verifying its HTTP signatures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey {
    id: String,
    owner: String,
    pem: String,
}

impl PublicKey {
    pub fn new(id: impl Into<String>, owner: impl Into<String>, pem: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            owner: owner.into(),
            pem: pem.into(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn pem(&self) -> &str {
        &self.pem
    }
}
