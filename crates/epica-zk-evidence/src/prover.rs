//! Ed25519-backed receipt prover.
//!
//! [`Ed25519Prover`] owns a signing key; calling [`Ed25519Prover::seal`]
//! produces a fresh [`EvidenceReceipt`] over a contiguous window of an
//! [`AuditLedger`][epica_contracts::AuditLedger]. The private key never
//! leaves the prover — verification uses only the public key embedded
//! in the receipt.

use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey, VerifyingKey, SECRET_KEY_LENGTH};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use tracing::debug;

use epica_contracts::AuditLedger;

use crate::receipt::{
    receipt_binding, EvidenceReceipt, MerkleInclusionProof, ReceiptMetadata,
    RECEIPT_SCHEMA_VERSION,
};

/// Errors raised by [`Ed25519Prover`] operations.
#[derive(Debug, thiserror::Error)]
pub enum ProverError {
    /// `start_seq > end_seq` or the range is out of bounds.
    #[error("invalid batch range: start={start}, end={end}, ledger_len={ledger_len}")]
    InvalidRange {
        /// Caller-supplied start.
        start: u64,
        /// Caller-supplied end.
        end: u64,
        /// Length of the ledger at sealing time.
        ledger_len: u64,
    },
    /// The supplied secret key bytes had the wrong length.
    #[error("secret key must be exactly {expected} bytes, got {got}")]
    SecretKeyLength {
        /// Required length.
        expected: usize,
        /// Observed length.
        got: usize,
    },
}

/// Signed-receipt producer.
///
/// Construct fresh with [`Self::generate`] (uses the OS RNG) or restore
/// from a previously-exported secret with [`Self::from_secret_bytes`].
/// Both paths derive the public key deterministically — restored
/// provers re-sign with the same identity.
pub struct Ed25519Prover {
    signing: SigningKey,
}

impl std::fmt::Debug for Ed25519Prover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Do NOT print the secret key. Public key only.
        f.debug_struct("Ed25519Prover")
            .field("public_key", &self.public_key_hex())
            .finish()
    }
}

impl Ed25519Prover {
    /// Generate a fresh keypair from the operating system's RNG.
    #[must_use]
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        Self { signing }
    }

    /// Restore a prover from previously-exported secret bytes
    /// (e.g. `signing_key.to_bytes()` on the producer side).
    pub fn from_secret_bytes(bytes: &[u8]) -> Result<Self, ProverError> {
        if bytes.len() != SECRET_KEY_LENGTH {
            return Err(ProverError::SecretKeyLength {
                expected: SECRET_KEY_LENGTH,
                got: bytes.len(),
            });
        }
        let mut buf = [0u8; SECRET_KEY_LENGTH];
        buf.copy_from_slice(bytes);
        Ok(Self {
            signing: SigningKey::from_bytes(&buf),
        })
    }

    /// Hex-encoded public key — what the verifier embeds.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.signing.verifying_key().to_bytes())
    }

    /// Raw public key bytes.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// Borrow the underlying [`VerifyingKey`] — used by tests and by
    /// callers that want to expose the prover identity in their own
    /// metadata structures.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// Export the secret bytes for storage. Use a key-management system
    /// in production — these bytes ARE the prover identity.
    pub fn export_secret_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.signing.to_bytes()
    }

    /// Seal the inclusive `[start_seq, end_seq]` window of `ledger` into
    /// an [`EvidenceReceipt`].
    pub fn seal(
        &self,
        ledger: &AuditLedger,
        start_seq: u64,
        end_seq: u64,
    ) -> Result<EvidenceReceipt, ProverError> {
        self.seal_with_options(ledger, start_seq, end_seq, &SealOptions::default())
    }

    /// Same as [`Self::seal`] but lets the caller embed extra goodies
    /// (label, per-entry inclusion proofs).
    pub fn seal_with_options(
        &self,
        ledger: &AuditLedger,
        start_seq: u64,
        end_seq: u64,
        opts: &SealOptions,
    ) -> Result<EvidenceReceipt, ProverError> {
        let ledger_len = ledger.len() as u64;
        if start_seq > end_seq || end_seq as usize >= ledger.len() {
            return Err(ProverError::InvalidRange {
                start: start_seq,
                end: end_seq,
                ledger_len,
            });
        }

        let merkle_root = window_merkle_root(ledger, start_seq, end_seq);
        let pubkey = self.public_key_bytes();
        let binding = receipt_binding(
            &merkle_root,
            start_seq,
            end_seq,
            ledger_len,
            &pubkey,
            RECEIPT_SCHEMA_VERSION,
        );
        let signature = self.signing.sign(&binding).to_bytes();

        let inclusions: Vec<MerkleInclusionProof> = opts
            .inclusions
            .iter()
            .filter_map(|seq| {
                let proof = ledger.merkle_proof(*seq)?;
                let entry = ledger.entries().get(*seq as usize)?;
                Some(MerkleInclusionProof {
                    seq: *seq,
                    self_hash_hex: hex::encode(entry.self_hash),
                    path_hex: proof.iter().map(hex::encode).collect(),
                })
            })
            .collect();

        let sealed_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        debug!(
            start_seq,
            end_seq,
            ledger_len,
            label = ?opts.label,
            inclusions = inclusions.len(),
            "epica-zk-evidence: sealed receipt"
        );

        Ok(EvidenceReceipt {
            meta: ReceiptMetadata {
                label: opts.label.clone(),
                sealed_at_ms,
                schema_version: RECEIPT_SCHEMA_VERSION,
            },
            merkle_root_hex: hex::encode(merkle_root),
            start_seq,
            end_seq,
            ledger_len,
            prover_pubkey_hex: hex::encode(pubkey),
            signature_hex: hex::encode(signature),
            inclusions,
        })
    }
}

/// Optional knobs for [`Ed25519Prover::seal_with_options`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SealOptions {
    /// Free-form label attached to [`ReceiptMetadata::label`].
    pub label: Option<String>,
    /// Sequence numbers within the window for which the prover should
    /// embed per-entry Merkle inclusion proofs. Out-of-range seqs are
    /// silently ignored. Empty = whole-batch verification only.
    pub inclusions: Vec<u64>,
}

/// Compute the Merkle root over a contiguous window of the ledger.
///
/// Equivalent to [`AuditLedger::merkle_root`] when the window covers
/// the whole ledger; for sub-windows it uses the same domain-separated
/// reduction restricted to `entries[start_seq..=end_seq]`. The verifier
/// reproduces this function bit-for-bit.
pub(crate) fn window_merkle_root(
    ledger: &AuditLedger,
    start_seq: u64,
    end_seq: u64,
) -> [u8; 32] {
    use blake3::Hasher;
    let s = start_seq as usize;
    let e = end_seq as usize;
    if e >= ledger.len() || s > e {
        return [0u8; 32];
    }
    let mut level: Vec<[u8; 32]> = ledger.entries()[s..=e]
        .iter()
        .map(|e| {
            let mut h = Hasher::new();
            h.update(&[0x00]);
            h.update(&e.self_hash);
            *h.finalize().as_bytes()
        })
        .collect();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let l = pair[0];
            let r = if pair.len() == 2 { pair[1] } else { pair[0] };
            let mut h = Hasher::new();
            h.update(&[0x01]);
            h.update(&l);
            h.update(&r);
            next.push(*h.finalize().as_bytes());
        }
        level = next;
    }
    level[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use epica_contracts::{AuditEntry, AuditEventType, AuditLedger};

    fn entry(key: &str) -> AuditEntry {
        AuditEntry {
            timestamp_ms: 1_700_000_000_000,
            event_type: AuditEventType::BeliefWrite,
            belief_key: Some(key.to_string()),
            agent_id: Some("A".into()),
            details: serde_json::json!({}),
        }
    }

    #[test]
    fn generate_produces_a_distinct_key_per_call() {
        let a = Ed25519Prover::generate();
        let b = Ed25519Prover::generate();
        assert_ne!(a.public_key_hex(), b.public_key_hex());
    }

    #[test]
    fn restore_from_secret_round_trips() {
        let p1 = Ed25519Prover::generate();
        let raw = p1.export_secret_bytes();
        let p2 = Ed25519Prover::from_secret_bytes(&raw).unwrap();
        assert_eq!(p1.public_key_hex(), p2.public_key_hex());
    }

    #[test]
    fn invalid_secret_length_is_rejected() {
        let err = Ed25519Prover::from_secret_bytes(&[0u8; 10]).unwrap_err();
        assert!(matches!(err, ProverError::SecretKeyLength { .. }));
    }

    #[test]
    fn seal_whole_ledger_matches_full_root() {
        let mut l = AuditLedger::new();
        for k in ["a", "b", "c", "d"] {
            l.append(entry(k));
        }
        let p = Ed25519Prover::generate();
        let r = p.seal(&l, 0, 3).unwrap();
        assert_eq!(r.merkle_root_hex, hex::encode(l.merkle_root()));
        assert_eq!(r.ledger_len, 4);
        assert_eq!(r.start_seq, 0);
        assert_eq!(r.end_seq, 3);
    }

    #[test]
    fn seal_rejects_out_of_range() {
        let mut l = AuditLedger::new();
        l.append(entry("only"));
        let p = Ed25519Prover::generate();
        assert!(matches!(
            p.seal(&l, 0, 99),
            Err(ProverError::InvalidRange { .. })
        ));
        assert!(matches!(
            p.seal(&l, 5, 6),
            Err(ProverError::InvalidRange { .. })
        ));
    }

    #[test]
    fn seal_with_options_attaches_inclusions() {
        let mut l = AuditLedger::new();
        for k in ["a", "b", "c", "d", "e"] {
            l.append(entry(k));
        }
        let p = Ed25519Prover::generate();
        let opts = SealOptions {
            label: Some("test-batch".into()),
            inclusions: vec![1, 3],
        };
        let r = p.seal_with_options(&l, 0, 4, &opts).unwrap();
        assert_eq!(r.inclusions.len(), 2);
        assert_eq!(r.meta.label.as_deref(), Some("test-batch"));
        // Inclusions ignore out-of-range gracefully.
        let opts2 = SealOptions {
            label: None,
            inclusions: vec![99, 100],
        };
        let r2 = p.seal_with_options(&l, 0, 4, &opts2).unwrap();
        assert_eq!(r2.inclusions.len(), 0);
    }
}
