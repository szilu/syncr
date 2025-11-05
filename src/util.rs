use sha::utils::{Digest, DigestExt};
use std::hash::Hasher;
use sha::sha1::Sha1 as Sha;

pub fn hash(buf: &[u8]) -> String {
    let mut hasher = Sha::default();
    hasher.digest(buf);
    let _ = hasher.finish();
    return hasher.to_hex();
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_hash() {
        let src: [u8;2 ] = ['1' as u8, '2' as u8];
        let res = hash(&src);
        // echo -n 12 | sha1sum
        assert_eq!(res, "7b52009b64fd0a2a49e6d8a939753077792b0554");
    }
}
