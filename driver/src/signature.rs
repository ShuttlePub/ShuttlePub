//! draft-cavage HTTP signatures as used by ActivityPub servers: `rsa-sha256`
//! over `(request-target) host date digest content-type`, with a SHA-256
//! `Digest` of the body. What verifying (`intake`) and signing (`delivery`)
//! share.

use http_msgsign_draft::digest::{ContentHasher, DigestHash};
use sha2::Sha256;

pub(crate) const ALGORITHM: &str = "rsa-sha256";
pub(crate) const REQUEST_TARGET: &str = "(request-target)";
pub(crate) const REQUIRED_COMPONENTS: [&str; 5] =
    [REQUEST_TARGET, "host", "date", "digest", "content-type"];

pub(crate) struct Sha256Hasher;

impl ContentHasher for Sha256Hasher {
    const DIGEST_ALG: &'static str = "SHA-256";

    fn hash(content: &[u8]) -> DigestHash {
        use sha2::Digest as _;
        DigestHash::new(Sha256::digest(content).to_vec())
    }
}
