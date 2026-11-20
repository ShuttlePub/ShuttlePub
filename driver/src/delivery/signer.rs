use std::time::SystemTime;

use bytes::Bytes;
use error_stack::{Report, ResultExt};
use http::{Request, header};
use http_body_util::Full;
use http_msgsign_draft::{
    digest::{Digest, body::Body},
    sign::{RequestSign, SignatureParams, SignerKey},
};
use kernel::KernelError;

use crate::{
    keys::DevelopmentKey,
    remote::ACTIVITY_JSON,
    signature::{ALGORITHM, REQUEST_TARGET, REQUIRED_COMPONENTS, Sha256Hasher},
};

/// Signs requests the local actor sends to other servers.
pub(crate) struct RequestSigner {
    key: DevelopmentKey,
    key_id: String,
}

impl RequestSigner {
    pub(crate) fn new(key: DevelopmentKey, key_id: String) -> Self {
        Self { key, key_id }
    }

    /// Builds a signed `POST` of an activity to `destination`.
    pub(crate) async fn sign_post(
        &self,
        destination: &str,
        body: Vec<u8>,
    ) -> Result<Request<Body>, Report<KernelError>> {
        let uri: http::Uri = destination
            .parse()
            .change_context(KernelError::UntrustedIdentity)?;
        let host = uri
            .authority()
            .ok_or_else(|| {
                Report::new(KernelError::UntrustedIdentity).attach("destination without authority")
            })?
            .to_string();
        let request = Request::post(uri)
            .header(header::HOST, host)
            .header(header::DATE, httpdate::fmt_http_date(SystemTime::now()))
            .header(header::CONTENT_TYPE, ACTIVITY_JSON)
            .body(Full::new(Bytes::from(body)))
            .change_context(KernelError::Malformed)?;
        let params = REQUIRED_COMPONENTS
            .into_iter()
            .fold(SignatureParams::builder(), |params, component| {
                if component == REQUEST_TARGET {
                    params.add_request_target()
                } else {
                    params.add_header(component)
                }
            })
            .build()
            .change_context(KernelError::InvalidSignature)?;
        request
            .digest::<Sha256Hasher>()
            .await
            .change_context(KernelError::InvalidSignature)?
            .sign(self, &params)
            .await
            .change_context(KernelError::InvalidSignature)
    }
}

impl SignerKey for RequestSigner {
    fn id(&self) -> String {
        self.key_id.clone()
    }

    fn algorithm(&self) -> String {
        ALGORITHM.to_owned()
    }

    fn sign(&self, target: &[u8]) -> Vec<u8> {
        self.key.sign(target)
    }
}
