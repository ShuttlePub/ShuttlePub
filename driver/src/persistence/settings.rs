use error_stack::Report;

use crate::DriverError;

const DATABASE_URL: &str = "SHUTTLEPUB_DATABASE_URL";

/// Where the PostgreSQL database is. Not `Debug`, and the URL is never
/// attached to an error: it carries the credentials.
pub struct DatabaseSettings {
    url: String,
}

impl DatabaseSettings {
    /// Reads `SHUTTLEPUB_DATABASE_URL` through `lookup` (`main` passes the
    /// process environment). There is no default: unset is an error.
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, Report<DriverError>> {
        let url = lookup(DATABASE_URL).ok_or_else(|| {
            Report::new(DriverError::InvalidSetting).attach(format!("{DATABASE_URL} is not set"))
        })?;
        Ok(Self { url })
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }
}
