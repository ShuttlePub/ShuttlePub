use error_stack::{Report, ResultExt};
use rsa::{
    RsaPrivateKey,
    pkcs1v15::SigningKey,
    pkcs8::{EncodePublicKey, LineEnding},
    rand_core::OsRng,
    signature::{SignatureEncoding, Signer},
};
use sha2::Sha256;

use crate::DriverError;

/// RSA key the local actor signs with. It only lives in memory: a new key is
/// generated at every start and never written anywhere.
#[derive(Clone)]
pub struct DevelopmentKey(RsaPrivateKey);

impl DevelopmentKey {
    /// Generates a 2048-bit key. Slow; run it off the async executor.
    pub fn generate() -> Result<Self, Report<DriverError>> {
        RsaPrivateKey::new(&mut OsRng, 2048)
            .change_context(DriverError::Key)
            .map(Self)
    }

    pub(crate) fn public_pem(&self) -> Result<String, Report<DriverError>> {
        self.0
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .change_context(DriverError::Key)
    }

    /// RSASSA-PKCS1-v1_5 with SHA-256, i.e. `rsa-sha256`.
    pub(crate) fn sign(&self, target: &[u8]) -> Vec<u8> {
        SigningKey::<Sha256>::new(self.0.clone())
            .sign(target)
            .to_vec()
    }
}
