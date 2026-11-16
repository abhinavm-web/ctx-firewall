use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixHash(pub [u8; 32]);

impl PrefixHash {
    pub fn compute(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hasher.finalize());
        Self(hash)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

#[derive(Debug)]
pub enum InvariantViolation {
    PrefixCorrupted { expected: String, actual: String },
    ByteMismatch,
}

impl fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrefixCorrupted { expected, actual } => {
                write!(
                    f,
                    "KV-cache prefix SHA-256 mismatch! Expected {}, got {}",
                    expected, actual
                )
            }
            Self::ByteMismatch => write!(f, "KV-cache prefix byte content mismatch"),
        }
    }
}

impl std::error::Error for InvariantViolation {}

pub struct SealedPrefixGuard {
    expected_hash: PrefixHash,
    canonical_sealed_bytes: Vec<u8>,
}

impl SealedPrefixGuard {
    pub fn seal(canonical_bytes: Vec<u8>) -> Self {
        let expected_hash = PrefixHash::compute(&canonical_bytes);
        Self {
            expected_hash,
            canonical_sealed_bytes: canonical_bytes,
        }
    }

    pub fn hash(&self) -> &PrefixHash {
        &self.expected_hash
    }

    pub fn verify_immutability(&self, candidate_bytes: &[u8]) -> Result<(), InvariantViolation> {
        let actual_hash = PrefixHash::compute(candidate_bytes);
        if self.expected_hash != actual_hash {
            return Err(InvariantViolation::PrefixCorrupted {
                expected: self.expected_hash.to_hex(),
                actual: actual_hash.to_hex(),
            });
        }
        if self.canonical_sealed_bytes != candidate_bytes {
            return Err(InvariantViolation::ByteMismatch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guard_verifies_identical_bytes() {
        let original = b"{\"system\": \"You are a helpful assistant.\"}".to_vec();
        let guard = SealedPrefixGuard::seal(original.clone());
        assert!(guard.verify_immutability(&original).is_ok());
    }

    #[test]
    fn test_guard_rejects_corrupted_bytes() {
        let original = b"{\"system\": \"You are a helpful assistant.\"}".to_vec();
        let guard = SealedPrefixGuard::seal(original);
        let mutated = b"{\"system\": \"You are an altered assistant.\"}".to_vec();
        assert!(guard.verify_immutability(&mutated).is_err());
    }
}
