use error_stack::{Report, ResultExt};
use url::{Position, Url};

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

    /// `host[:port]` of the actor URL: the service the actor belongs to, and
    /// what a request addressed to it carries in its `Host` header.
    pub fn authority(&self) -> &str {
        &self.url[Position::BeforeHost..Position::AfterPort]
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

/// Identifier of a received ActivityStreams object (an activity, or the Note
/// a Create carries); guaranteed to be a URL, kept verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityId {
    raw: String,
    url: Url,
}

impl ActivityId {
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
