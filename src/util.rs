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
    fn test_hash_simple() {
        let src: [u8; 2] = ['1' as u8, '2' as u8];
        let res = hash(&src);
        // echo -n 12 | sha1sum
        assert_eq!(res, "7b52009b64fd0a2a49e6d8a939753077792b0554");
    }

    #[test]
    fn test_hash_empty() {
        let src: [u8; 0] = [];
        let res = hash(&src);
        // echo -n "" | sha1sum
        assert_eq!(res, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn test_hash_longer_text() {
        let src = b"The quick brown fox jumps over the lazy dog";
        let res = hash(src);
        // echo -n "The quick brown fox jumps over the lazy dog" | sha1sum
        assert_eq!(res, "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12");
    }

    #[test]
    fn test_hash_binary() {
        let src: [u8; 4] = [0x00, 0xFF, 0xDE, 0xAD];
        let res = hash(&src);
        // Binary data should produce consistent hash
        assert_eq!(res.len(), 40); // SHA1 is 40 hex chars
    }

    #[test]
    fn test_hash_consistency() {
        let src = b"test data";
        let res1 = hash(src);
        let res2 = hash(src);
        assert_eq!(res1, res2, "Hash should be deterministic");
    }

    #[test]
    fn test_hash_different_inputs() {
        let src1 = b"test1";
        let src2 = b"test2";
        let res1 = hash(src1);
        let res2 = hash(src2);
        assert_ne!(res1, res2, "Different inputs should produce different hashes");
    }
}
