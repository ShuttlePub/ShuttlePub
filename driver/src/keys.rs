use http_msgsign_draft::{
    errors::VerificationError,
    sign::{SignerKey, VerifierKey},
};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::{Signature, SigningKey, VerifyingKey},
    pkcs8::{DecodePublicKey, EncodePublicKey, LineEnding},
    rand_core::OsRng,
    signature::{SignatureEncoding, Signer, Verifier},
};
use sha2::Sha256;

use crate::FederationError;

#[derive(Clone)]
pub struct DevelopmentKey(RsaPrivateKey);

impl DevelopmentKey {
    pub fn generate() -> Result<Self, FederationError> {
        Ok(Self(RsaPrivateKey::new(&mut OsRng, 2048)?))
    }

    pub fn public_pem(&self) -> Result<String, FederationError> {
        Ok(self.0.to_public_key().to_public_key_pem(LineEnding::LF)?)
    }
}

pub struct SigningIdentity {
    pub key: DevelopmentKey,
    pub id: String,
}

impl SignerKey for SigningIdentity {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn algorithm(&self) -> String {
        "rsa-sha256".into()
    }
    fn sign(&self, target: &[u8]) -> Vec<u8> {
        SigningKey::<Sha256>::new(self.key.0.clone())
            .sign(target)
            .to_vec()
    }
}

pub struct VerificationIdentity {
    id: String,
    key: VerifyingKey<Sha256>,
}

impl VerificationIdentity {
    pub fn new(id: &str, pem: &str) -> Result<Self, FederationError> {
        Ok(Self {
            id: id.into(),
            key: VerifyingKey::new(RsaPublicKey::from_public_key_pem(pem)?),
        })
    }
}

impl VerifierKey for VerificationIdentity {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn algorithm(&self) -> String {
        "rsa-sha256".into()
    }
    fn verify(&self, target: &[u8], signature: &[u8]) -> Result<(), VerificationError> {
        let signature = Signature::try_from(signature)
            .map_err(|error| VerificationError::Crypto(Box::new(error)))?;
        self.key
            .verify(target, &signature)
            .map_err(|error| VerificationError::Crypto(Box::new(error)))
    }
}
