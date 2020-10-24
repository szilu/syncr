use sha::utils::{Digest, DigestExt};
use std::hash::Hasher;
use sha::sha1::Sha1 as Sha;

pub fn hash(buf: &[u8]) -> String {
    let mut hasher = Sha::default();
    hasher.digest(buf);
    hasher.finish();
    return hasher.to_hex();
}
