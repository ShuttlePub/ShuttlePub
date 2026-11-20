use std::{collections::BTreeSet, fmt};

use error_stack::{Report, ResultExt};
use url::Url;

use crate::DriverError;

const BIND: &str = "SHUTTLEPUB_BIND";
const ACTOR: &str = "SHUTTLEPUB_ACTOR";
const REMOTE_ORIGINS: &str = "SHUTTLEPUB_REMOTE_ORIGINS";
const DATABASE_URL: &str = "SHUTTLEPUB_DATABASE_URL";
const DEFAULT_BIND: &str = "127.0.0.1:3000";
const DEFAULT_ACTOR: &str = "http://127.0.0.1:3000/actor";

/// Settings read once at startup.
pub struct Config {
    bind: String,
    actor: Url,
    remote_origins: RemoteOrigins,
    database_url: Option<String>,
}

impl Config {
    /// Reads `SHUTTLEPUB_BIND`, `SHUTTLEPUB_ACTOR`,
    /// `SHUTTLEPUB_REMOTE_ORIGINS` and `SHUTTLEPUB_DATABASE_URL` through
    /// `lookup` (`main` passes the process environment). Unset settings take
    /// their defaults, except the database URL, which has none; an actor or
    /// origin that is not a URL is an error rather than being ignored.
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, Report<DriverError>> {
        let bind = lookup(BIND).unwrap_or_else(|| DEFAULT_BIND.to_owned());
        let actor = lookup(ACTOR).unwrap_or_else(|| DEFAULT_ACTOR.to_owned());
        let actor = Url::parse(&actor)
            .change_context(DriverError::InvalidSetting)
            .attach(format!("{ACTOR}={actor}"))?;
        let remote_origins = RemoteOrigins::parse(&lookup(REMOTE_ORIGINS).unwrap_or_default())?;
        Ok(Self {
            bind,
            actor,
            remote_origins,
            database_url: lookup(DATABASE_URL),
        })
    }

    pub fn bind(&self) -> &str {
        &self.bind
    }

    pub fn actor(&self) -> &Url {
        &self.actor
    }

    pub fn remote_origins(&self) -> &RemoteOrigins {
        &self.remote_origins
    }

    /// The PostgreSQL to keep the events in, if one is configured. Whether one
    /// is required is up to what is being assembled.
    pub fn database_url(&self) -> Option<&str> {
        self.database_url.as_deref()
    }
}

/// The database URL may carry a password, so it is never printed.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("bind", &self.bind)
            .field("actor", &self.actor)
            .field("remote_origins", &self.remote_origins)
            .field(
                "database_url",
                &self.database_url.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Origins this server may contact. Empty means no remote access at all.
#[derive(Clone, Debug)]
pub struct RemoteOrigins(BTreeSet<String>);

impl RemoteOrigins {
    /// Comma-separated URLs; empty entries are skipped and each entry is
    /// reduced to its origin, so a path after the origin is ignored.
    fn parse(list: &str) -> Result<Self, Report<DriverError>> {
        list.split(',')
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                Url::parse(entry)
                    .map(|url| url.origin().ascii_serialization())
                    .change_context(DriverError::InvalidSetting)
                    .attach(format!("{REMOTE_ORIGINS} entry {entry}"))
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// Whether `url` is an http(s) URL without credentials on a listed origin.
    pub fn permits(&self, url: &Url) -> bool {
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
            && self.0.contains(&url.origin().ascii_serialization())
    }
}
