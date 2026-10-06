use sha1::{Digest, Sha1};
use sha2::Sha256;

const SHA1_WIDTH: usize = 20;
const SHA256_WIDTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Checksums {
    sha1: bool,
    sha256: bool,
}

impl Checksums {
    pub(super) fn calculate(data: &[u8]) -> Self {
        Self {
            sha1: sha1_matches(data),
            sha256: sha256_matches(data),
        }
    }

    pub(super) fn sha1_matches(self) -> bool {
        self.sha1
    }

    pub(super) fn sha256_matches(self) -> bool {
        self.sha256
    }
}

fn sha1_matches(data: &[u8]) -> bool {
    let Some(body_end) = data.len().checked_sub(SHA1_WIDTH) else {
        return false;
    };
    let digest = Sha1::digest(&data[..body_end]);
    digest[..] == data[body_end..]
}

fn sha256_matches(data: &[u8]) -> bool {
    let Some(body_end) = data.len().checked_sub(SHA256_WIDTH) else {
        return false;
    };
    let digest = Sha256::digest(&data[..body_end]);
    digest[..] == data[body_end..]
}
