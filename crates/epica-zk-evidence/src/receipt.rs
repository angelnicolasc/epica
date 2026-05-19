//! Wire format for [`EvidenceReceipt`] and its building blocks.
//!
//! The receipt is a small, fully-serialisable JSON document that travels
//! independently of the ledger it commits to. Verifying it requires the
//! ledger entries — but those are *public* (every governance event has
//! the same emission policy), so the receipt's role is to prove the
//! ledger snapshot wasn't tampered with after the prover signed it.
//!
//! Hex encoding is used for every binary field so the JSON stays
//! human-readable and pipe-friendly (`epica-verify ledger.json
//! receipt.json` is the intended UX).

use serde::{Deserialize, Serialize};

/// Header attached to every receipt — what was sealed, when, and by whom.
///
/// Kept deliberately small. Anything richer (free-form labels,
/// human-readable description, etc.) goes in [`ReceiptMetadata`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceiptMetadata {
    /// Optional free-form label. CLI tools may print it; verification
    /// ignores it.
    pub label: Option<String>,
    /// Wall-clock millisecond timestamp at sealing time. Informational —
    /// the cryptographic binding uses only the Merkle root, range, and
    /// public key.
    pub sealed_at_ms: u64,
    /// Schema version of this receipt. Bumped on breaking wire-format
    /// changes; the `Ed25519Verifier` refuses to verify unknown
    /// versions.
    pub schema_version: u32,
}

/// Per-entry Merkle inclusion proof carried inside a receipt.
///
/// When attached, `verify` runs an extra check that the entry at `seq`
/// hashes into the receipt's `merkle_root`. Useful when the auditor
/// only has access to one entry (e.g. a redacted ledger excerpt) and
/// wants to confirm its inclusion without seeing the rest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerkleInclusionProof {
    /// 0-based sequence number of the proven entry.
    pub seq: u64,
    /// Hex-encoded `self_hash` of the entry.
    pub self_hash_hex: String,
    /// Hex-encoded sibling hashes from leaf to root, in order.
    pub path_hex: Vec<String>,
}

/// A signed commitment to a contiguous slice of an audit ledger.
///
/// Verification cost: one Ed25519 signature check + one Merkle root
/// recomputation across `[start_seq, end_seq]`. No network, no zkVM, no
/// trusted setup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceReceipt {
    /// Metadata header.
    pub meta: ReceiptMetadata,

    /// Hex-encoded BLAKE3 Merkle root over `[start_seq, end_seq]` of the
    /// sealed ledger. Identical to
    /// `AuditLedger::merkle_root()` when the receipt covers the whole
    /// ledger; otherwise computed on the sub-slice using the same
    /// domain-separated reduction.
    pub merkle_root_hex: String,

    /// 0-based inclusive start of the sealed range.
    pub start_seq: u64,

    /// 0-based inclusive end of the sealed range.
    pub end_seq: u64,

    /// Length of the underlying ledger at sealing time (used to
    /// reproduce the odd-tail duplication rule when verifying).
    pub ledger_len: u64,

    /// Hex-encoded Ed25519 public key of the prover. Verifiers use this
    /// directly — they MUST cross-reference it against a trusted
    /// directory before accepting the receipt as authoritative
    /// (otherwise the receipt only proves "*some* key signed this",
    /// which is necessary but not sufficient for accountability).
    pub prover_pubkey_hex: String,

    /// Hex-encoded Ed25519 signature over
    /// [`receipt_binding`](crate::receipt::receipt_binding).
    pub signature_hex: String,

    /// Optional per-entry Merkle inclusion proofs. Empty when not
    /// requested — the receipt alone suffices for whole-batch
    /// verification.
    #[serde(default)]
    pub inclusions: Vec<MerkleInclusionProof>,
}

/// Schema version of this implementation. Bump on breaking changes.
pub const RECEIPT_SCHEMA_VERSION: u32 = 1;

/// Errors emitted when *parsing* a receipt — distinct from verification
/// errors (see [`crate::VerificationError`]). A receipt that fails to
/// parse never reaches the verifier.
#[derive(Debug, thiserror::Error)]
pub enum EvidenceReceiptError {
    /// One of the hex-encoded fields had an invalid encoding.
    #[error("invalid hex encoding in field {field}: {source}")]
    Hex {
        /// Which JSON field was being decoded.
        field: &'static str,
        /// Underlying decoder error.
        source: hex::FromHexError,
    },
    /// A fixed-length field had the wrong byte length after hex decoding.
    #[error("field {field} expected {expected} bytes, got {got}")]
    InvalidLength {
        /// Which field.
        field: &'static str,
        /// Required length in bytes.
        expected: usize,
        /// Observed length.
        got: usize,
    },
}

/// Compute the canonical signing payload for a receipt.
///
/// `BLAKE3(merkle_root || start_seq_le || end_seq_le || ledger_len_le ||
///        prover_pubkey || schema_version_le)`.
///
/// Domain separation by encoding (all little-endian, fixed lengths)
/// makes this trivially unambiguous and matches what the verifier
/// reconstructs. The function is part of the public API because
/// downstream provers (e.g. future RISC Zero guests) need to commit to
/// the exact same bytes.
#[must_use]
pub fn receipt_binding(
    merkle_root: &[u8; 32],
    start_seq: u64,
    end_seq: u64,
    ledger_len: u64,
    prover_pubkey: &[u8; 32],
    schema_version: u32,
) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"epica-evidence-receipt-v1\0");
    h.update(merkle_root);
    h.update(&start_seq.to_le_bytes());
    h.update(&end_seq.to_le_bytes());
    h.update(&ledger_len.to_le_bytes());
    h.update(prover_pubkey);
    h.update(&schema_version.to_le_bytes());
    *h.finalize().as_bytes()
}

/// Decode a hex string into a fixed-length byte array. Used by both
/// [`Ed25519Verifier`][crate::Ed25519Verifier] and the
/// [`epica-verify`][1] CLI.
///
/// [1]: https://github.com/angelnicolasc/epica
pub(crate) fn decode_fixed_hex<const N: usize>(
    hex_str: &str,
    field: &'static str,
) -> Result<[u8; N], EvidenceReceiptError> {
    let raw = hex::decode(hex_str).map_err(|source| EvidenceReceiptError::Hex { field, source })?;
    if raw.len() != N {
        return Err(EvidenceReceiptError::InvalidLength {
            field,
            expected: N,
            got: raw.len(),
        });
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&raw);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_is_deterministic() {
        let root = [7u8; 32];
        let pk = [3u8; 32];
        let a = receipt_binding(&root, 0, 9, 10, &pk, 1);
        let b = receipt_binding(&root, 0, 9, 10, &pk, 1);
        assert_eq!(a, b);
    }

    #[test]
    fn binding_changes_under_any_field_perturbation() {
        let root = [7u8; 32];
        let pk = [3u8; 32];
        let base = receipt_binding(&root, 0, 9, 10, &pk, 1);
        let mut root2 = root;
        root2[0] ^= 1;
        assert_ne!(base, receipt_binding(&root2, 0, 9, 10, &pk, 1));
        assert_ne!(base, receipt_binding(&root, 1, 9, 10, &pk, 1));
        assert_ne!(base, receipt_binding(&root, 0, 8, 10, &pk, 1));
        assert_ne!(base, receipt_binding(&root, 0, 9, 11, &pk, 1));
        let mut pk2 = pk;
        pk2[0] ^= 1;
        assert_ne!(base, receipt_binding(&root, 0, 9, 10, &pk2, 1));
        assert_ne!(base, receipt_binding(&root, 0, 9, 10, &pk, 2));
    }

    #[test]
    fn decode_fixed_hex_round_trip() {
        let bytes = [0xaa_u8; 32];
        let s = hex::encode(bytes);
        let back: [u8; 32] = decode_fixed_hex(&s, "test").unwrap();
        assert_eq!(back, bytes);
    }

    #[test]
    fn decode_fixed_hex_rejects_wrong_length() {
        let s = hex::encode([0u8; 31]);
        let r: Result<[u8; 32], _> = decode_fixed_hex(&s, "test");
        assert!(matches!(
            r,
            Err(EvidenceReceiptError::InvalidLength {
                expected: 32,
                got: 31,
                ..
            })
        ));
    }
}
