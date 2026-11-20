use std::time::Duration;

use error_stack::{Report, ResultExt};
use kernel::KernelError;
use url::Url;

use crate::{DriverError, config::RemoteOrigins, delivery::RequestSigner, keys::DevelopmentKey};

pub(crate) const ACTIVITY_JSON: &str = "application/activity+json";

/// HTTP client for other servers: resolves actors (`intake`) and delivers
/// Accepts (`delivery`), only to allowed origins, without redirects or ambient
/// proxies.
pub struct RemoteClient {
    pub(crate) http: reqwest::Client,
    origins: RemoteOrigins,
    pub(crate) signer: RequestSigner,
}

impl RemoteClient {
    /// `key_id` is the id under which the local actor publishes `key`.
    pub fn new(
        origins: RemoteOrigins,
        key: DevelopmentKey,
        key_id: String,
    ) -> Result<Self, Report<DriverError>> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .change_context(DriverError::HttpClient)?;
        Ok(Self {
            http,
            origins,
            signer: RequestSigner::new(key, key_id),
        })
    }

    pub(crate) fn destination(&self, url: Url) -> Result<Url, Report<KernelError>> {
        if !self.origins.permits(&url) {
            return Err(Report::new(KernelError::OriginNotAllowed)
                .attach(format!("origin {}", url.origin().ascii_serialization())));
        }
        Ok(url)
    }
}
