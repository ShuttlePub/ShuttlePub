use error_stack::{Report, ResultExt};
use kernel::community::{LocalActor, LocalActorProvider};
use url::Url;

use crate::{DriverError, keys::DevelopmentKey};

/// The configured local actor, publishing the public half of the signing key.
pub struct LocalIdentity(LocalActor);

impl LocalIdentity {
    pub fn new(actor: Url, key: &DevelopmentKey) -> Result<Self, Report<DriverError>> {
        let actor = LocalActor::new(actor, key.public_pem()?)
            .change_context(DriverError::InvalidSetting)?;
        Ok(Self(actor))
    }
}

impl LocalActorProvider for LocalIdentity {
    fn local_actor(&self) -> &LocalActor {
        &self.0
    }
}
