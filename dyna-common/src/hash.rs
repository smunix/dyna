//! Content-addressable hashing utilities.
//!
//! All patches, changesets, and snapshots in Dyna are identified by the SHA-256
//! hash of their serialized content. This module provides the hashing functions
//! used throughout the system for computing `commit_hash` values on changesets
//! and `hash` values on patches.

use sha2::{Sha256, Digest};

/// Compute the SHA-256 hash of a byte slice and return it as a hex string.
///
/// # Example
/// ```
/// let hash = dyna_common::hash::sha256_hex(b"hello world");
/// assert_eq!(hash.len(), 64); // 256 bits = 64 hex chars
/// ```
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Compute the content-addressable hash for a patch.
///
/// The hash is computed over the canonical JSON serialization of the patch
/// content (excluding the hash field itself). Returns a prefixed string like
/// `sha256:abcdef...`.
pub fn content_hash(data: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(data))
}

/// Verify that the given data matches the expected content hash.
pub fn verify_hash(data: &[u8], expected: &str) -> bool {
    content_hash(data) == expected
}

/// Extract the raw hex portion from a prefixed hash string.
/// e.g., `sha256:abcdef...` -> `abcdef...`
pub fn strip_prefix(hash: &str) -> &str {
    hash.strip_prefix("sha256:").unwrap_or(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_content_hash() {
        let hash = content_hash(b"test data");
        assert!(hash.starts_with("sha256:"));
    }

    #[test]
    fn test_verify_hash() {
        let data = b"test data";
        let hash = content_hash(data);
        assert!(verify_hash(data, &hash));
        assert!(!verify_hash(b"wrong data", &hash));
    }

    #[test]
    fn test_strip_prefix() {
        assert_eq!(strip_prefix("sha256:abc123"), "abc123");
        assert_eq!(strip_prefix("abc123"), "abc123");
    }
}
