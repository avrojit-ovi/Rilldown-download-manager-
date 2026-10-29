//! Streaming checksum verification.
//!
//! Hashing never loads the whole file into memory. A mismatch returns both the
//! expected and actual digests so the UI can show what happened.

use std::path::Path;

use sha2::Digest;
use tokio::io::AsyncReadExt;

use crate::error::{EngineError, EngineResult};
use crate::types::{Checksum, ChecksumAlgorithm};

/// Verify a file against an expected digest, returning the actual hex digest.
pub async fn verify_file(path: &Path, checksum: &Checksum) -> EngineResult<String> {
    let actual = match checksum.algorithm {
        ChecksumAlgorithm::Md5 => digest_file::<md5::Md5>(path).await?,
        ChecksumAlgorithm::Sha1 => digest_file::<sha1::Sha1>(path).await?,
        ChecksumAlgorithm::Sha256 => digest_file::<sha2::Sha256>(path).await?,
    };
    if actual != checksum.expected_hex.trim().to_ascii_lowercase() {
        return Err(EngineError::ChecksumMismatch {
            expected: checksum.expected_hex.clone(),
            actual,
        });
    }
    Ok(actual)
}

/// Hash a file with the given digest implementation.
pub async fn digest_file<D>(path: &Path) -> EngineResult<String>
where
    D: Digest + Default,
{
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = D::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn digest_of(algorithm: ChecksumAlgorithm, contents: &[u8]) -> String {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload.bin");
        tokio::fs::write(&path, contents).await.unwrap();
        match algorithm {
            ChecksumAlgorithm::Md5 => digest_file::<md5::Md5>(&path).await.unwrap(),
            ChecksumAlgorithm::Sha1 => digest_file::<sha1::Sha1>(&path).await.unwrap(),
            ChecksumAlgorithm::Sha256 => digest_file::<sha2::Sha256>(&path).await.unwrap(),
        }
    }

    #[tokio::test]
    async fn known_digests() {
        assert_eq!(
            digest_of(ChecksumAlgorithm::Md5, b"abc").await,
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            digest_of(ChecksumAlgorithm::Sha1, b"abc").await,
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            digest_of(ChecksumAlgorithm::Sha256, b"abc").await,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn mismatch_is_reported_with_both_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload.bin");
        tokio::fs::write(&path, b"actual").await.unwrap();
        let checksum = Checksum {
            algorithm: ChecksumAlgorithm::Sha256,
            expected_hex: "00".repeat(32),
        };
        match verify_file(&path, &checksum).await.unwrap_err() {
            EngineError::ChecksumMismatch { expected, actual } => {
                assert_eq!(expected, "00".repeat(32));
                assert_ne!(actual, expected);
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
