use error_stack::{Report, ResultExt};
use url::Url;

use crate::KernelError;

/// Identifier of an actor. Keeps the exact string it was given, because
/// identities are compared as received rather than after URL normalisation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorId {
    raw: String,
    url: Url,
}

impl ActorId {
    pub fn parse(raw: impl Into<String>) -> Result<Self, Report<KernelError>> {
        let raw = raw.into();
        let url = Url::parse(&raw).change_context(KernelError::Malformed)?;
        Ok(Self { raw, url })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn as_url(&self) -> &Url {
        &self.url
    }
}

impl From<Url> for ActorId {
    fn from(url: Url) -> Self {
        Self {
            raw: url.as_str().to_owned(),
            url,
        }
    }
}

/// Identifier of a received activity; guaranteed to be a URL, kept verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityId(String);

impl ActivityId {
    pub fn parse(raw: impl Into<String>) -> Result<Self, Report<KernelError>> {
        let raw = raw.into();
        Url::parse(&raw).change_context(KernelError::Malformed)?;
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
