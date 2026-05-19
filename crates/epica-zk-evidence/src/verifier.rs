//! Receipt verification.
//!
//! Stateless: given an [`EvidenceReceipt`] and the underlying
//! [`AuditLedger`][epica_contracts::AuditLedger], [`Ed25519Verifier`]
//! returns `Ok(())` iff every layer of the receipt's claim holds:
//!
//! 1. The receipt parses (correct field lengths, supported schema).
//! 2. The Merkle root recomputed from the ledger window matches the
//!    receipt's declared root.
//! 3. The Ed25519 signature verifies against
//!    [`receipt_binding`](crate::receipt::receipt_binding) under the
//!    public key embedded in the receipt.
//! 4. Every per-entry inclusion proof (if any) reconstructs the same
//!    root.
//!
//! Verification does NOT cross-check the public key against any
//! identity directory — that's an out-of-band step the auditor performs
//! before trusting the receipt as authoritative.

use ed25519_dalek::{Signature, Verifier, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};

use epica_contracts::{verify_merkle_proof as verify_inclusion, AuditLedger};

use crate::prover::window_merkle_root;
use crate::receipt::{
    decode_fixed_hex, receipt_binding, EvidenceReceipt, EvidenceReceiptError,
    RECEIPT_SCHEMA_VERSION,
};

/// Errors raised when a receipt fails to verify.
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    /// The receipt's wire format is malformed.
    #[error("receipt decode error: {0}")]
    Decode(#[from] EvidenceReceiptError),

    /// `schema_version` in the receipt is not understood by this
    /// verifier build.
    #[error("unsupported schema version: {0} (this build supports {1})")]
    UnsupportedSchemaVersion(u32, u32),

    /// `start_seq > end_seq` or the range extends past the ledger.
    #[error("range invalid for ledger: start={start}, end={end}, ledger_len={ledger_len}")]
    InvalidRange {
        /// Range start declared on the receipt.
        start: u64,
        /// Range end declared on the receipt.
        end: u64,
        /// Current length of the ledger we're verifying against.
        ledger_len: u64,
    },

    /// The ledger we were handed has a different length than the
    /// receipt was sealed against. Could indicate either truncation or
    /// the wrong file passed to the CLI.
    #[error("ledger length mismatch: receipt sealed at len={sealed}, current len={current}")]
    LedgerLengthMismatch {
        /// Length recorded on the receipt.
        sealed: u64,
        /// Length observed at verification time.
        current: u64,
    },

    /// The Merkle root the verifier reconstructs from the ledger window
    /// does not match the root the prover signed.
    #[error("merkle root mismatch: receipt={receipt}, reconstructed={reconstructed}")]
    MerkleRootMismatch {
        /// Hex of the receipt's declared root.
        receipt: String,
        /// Hex of the root we recomputed.
        reconstructed: String,
    },

    /// The receipt's Ed25519 signature does not verify against the
    /// reconstructed signing payload. Most common cause of this in
    /// practice: someone tampered with one of the receipt's other
    /// fields after sealing.
    #[error("signature verification failed: {0}")]
    Signature(#[from] ed25519_dalek::SignatureError),

    /// One of the per-entry inclusion proofs failed to reconstruct the
    /// root.
    #[error("inclusion proof failed for seq {0}")]
    Inclusion(u64),

    /// The receipt cites an entry seq that no longer exists in the
    /// ledger.
    #[error("inclusion seq {0} is out of bounds for the supplied ledger")]
    InclusionOutOfBounds(u64),

    /// The receipt's leaf hash disagrees with what's currently in the
    /// ledger at that position.
    #[error("inclusion leaf hash mismatch at seq {0}")]
    InclusionLeafMismatch(u64),
}

/// Stateless receipt verifier.
///
/// In the common case construct via [`Self::from_receipt`] — the
/// public key is read straight off the receipt. For higher-assurance
/// workflows (cross-checking against a known-good directory), construct
/// via [`Self::with_pubkey`] and pass the audited key explicitly: any
/// mismatch between the audited key and the receipt's embedded key is
/// then caught by the signature check itself.
pub struct Ed25519Verifier {
    pubkey: VerifyingKey,
}

impl Ed25519Verifier {
    /// Build a verifier whose key comes from the receipt.
    pub fn from_receipt(receipt: &EvidenceReceipt) -> Result<Self, VerificationError> {
        let pk: [u8; PUBLIC_KEY_LENGTH] =
            decode_fixed_hex(&receipt.prover_pubkey_hex, "prover_pubkey_hex")?;
        let pubkey = VerifyingKey::from_bytes(&pk)?;
        Ok(Self { pubkey })
    }

    /// Build a verifier with a caller-trusted public key. When the
    /// receipt's embedded key disagrees, signature verification fails —
    /// no extra check needed.
    pub fn with_pubkey(pubkey: VerifyingKey) -> Self {
        Self { pubkey }
    }

    /// Verify a receipt against the supplied ledger.
    ///
    /// `Ok(())` means every layer (schema, range, root, signature,
    /// inclusions) checked out. Any other result names the layer that
    /// failed.
    pub fn verify(
        &self,
        receipt: &EvidenceReceipt,
        ledger: &AuditLedger,
    ) -> Result<(), VerificationError> {
        // Layer 1 — schema version.
        if receipt.meta.schema_version != RECEIPT_SCHEMA_VERSION {
            return Err(VerificationError::UnsupportedSchemaVersion(
                receipt.meta.schema_version,
                RECEIPT_SCHEMA_VERSION,
            ));
        }

        // Layer 1 — length match first: tells "wrong ledger" from
        // "garbage range" cleanly.
        let ledger_len = ledger.len() as u64;
        if receipt.ledger_len != ledger_len {
            return Err(VerificationError::LedgerLengthMismatch {
                sealed: receipt.ledger_len,
                current: ledger_len,
            });
        }
        if receipt.start_seq > receipt.end_seq
            || receipt.end_seq as usize >= ledger.len()
        {
            return Err(VerificationError::InvalidRange {
                start: receipt.start_seq,
                end: receipt.end_seq,
                ledger_len,
            });
        }

        // Layer 2 — reconstruct the Merkle root from the window.
        let reconstructed =
            window_merkle_root(ledger, receipt.start_seq, receipt.end_seq);
        let receipt_root: [u8; 32] =
            decode_fixed_hex(&receipt.merkle_root_hex, "merkle_root_hex")?;
        if reconstructed != receipt_root {
            return Err(VerificationError::MerkleRootMismatch {
                receipt: hex::encode(receipt_root),
                reconstructed: hex::encode(reconstructed),
            });
        }

        // Layer 3 — signature.
        let pubkey_bytes: [u8; PUBLIC_KEY_LENGTH] =
            decode_fixed_hex(&receipt.prover_pubkey_hex, "prover_pubkey_hex")?;
        let binding = receipt_binding(
            &reconstructed,
            receipt.start_seq,
            receipt.end_seq,
            receipt.ledger_len,
            &pubkey_bytes,
            receipt.meta.schema_version,
        );
        let sig_bytes: [u8; SIGNATURE_LENGTH] =
            decode_fixed_hex(&receipt.signature_hex, "signature_hex")?;
        let signature = Signature::from_bytes(&sig_bytes);
        self.pubkey.verify(&binding, &signature)?;

        // Layer 4 — per-entry inclusion proofs (optional).
        for inc in &receipt.inclusions {
            if (inc.seq as usize) >= ledger.len() {
                return Err(VerificationError::InclusionOutOfBounds(inc.seq));
            }
            let observed_leaf = ledger.entries()[inc.seq as usize].self_hash;
            let claimed_leaf: [u8; 32] =
                decode_fixed_hex(&inc.self_hash_hex, "inclusion.self_hash_hex")?;
            if observed_leaf != claimed_leaf {
                return Err(VerificationError::InclusionLeafMismatch(inc.seq));
            }
            let path: Vec<[u8; 32]> = inc
                .path_hex
                .iter()
                .map(|s| decode_fixed_hex::<32>(s, "inclusion.path"))
                .collect::<Result<_, _>>()?;
            // Inclusion proofs are produced against the full-ledger root,
            // not the window root. That's intentional: the path tells an
            // outside auditor "this leaf is in the ledger at position
            // seq", independent of which window the receipt sealed.
            let full_root = ledger.merkle_root();
            if !verify_inclusion(&claimed_leaf, inc.seq, ledger_len, &path, &full_root) {
                return Err(VerificationError::Inclusion(inc.seq));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prover::{Ed25519Prover, SealOptions};
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

    fn ledger_of(n: usize) -> AuditLedger {
        let mut l = AuditLedger::new();
        for i in 0..n {
            l.append(entry(&format!("k{i}")));
        }
        l
    }

    #[test]
    fn happy_path_round_trip() {
        let l = ledger_of(8);
        let p = Ed25519Prover::generate();
        let r = p.seal(&l, 0, 7).unwrap();
        let v = Ed25519Verifier::from_receipt(&r).unwrap();
        v.verify(&r, &l).expect("happy path");
    }

    #[test]
    fn happy_path_partial_window() {
        let l = ledger_of(16);
        let p = Ed25519Prover::generate();
        let r = p.seal(&l, 4, 11).unwrap();
        let v = Ed25519Verifier::from_receipt(&r).unwrap();
        v.verify(&r, &l).unwrap();
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let l = ledger_of(4);
        let p = Ed25519Prover::generate();
        let mut r = p.seal(&l, 0, 3).unwrap();
        // Flip a bit in the signature hex.
        let mut bytes = hex::decode(&r.signature_hex).unwrap();
        bytes[0] ^= 1;
        r.signature_hex = hex::encode(bytes);

        let v = Ed25519Verifier::from_receipt(&r).unwrap();
        let err = v.verify(&r, &l).unwrap_err();
        assert!(matches!(err, VerificationError::Signature(_)));
    }

    #[test]
    fn tampered_ledger_entry_is_rejected() {
        let l = ledger_of(4);
        let p = Ed25519Prover::generate();
        let r = p.seal(&l, 0, 3).unwrap();

        // Build a second ledger that differs in one entry.
        let mut l2 = AuditLedger::new();
        for i in 0..4 {
            let mut e = entry(&format!("k{i}"));
            if i == 2 {
                e.belief_key = Some("forged".into());
            }
            l2.append(e);
        }
        let v = Ed25519Verifier::from_receipt(&r).unwrap();
        let err = v.verify(&r, &l2).unwrap_err();
        assert!(matches!(err, VerificationError::MerkleRootMismatch { .. }));
    }

    #[test]
    fn shorter_ledger_is_rejected() {
        let l = ledger_of(8);
        let p = Ed25519Prover::generate();
        let r = p.seal(&l, 0, 7).unwrap();

        let l2 = ledger_of(7);
        let v = Ed25519Verifier::from_receipt(&r).unwrap();
        let err = v.verify(&r, &l2).unwrap_err();
        assert!(matches!(err, VerificationError::LedgerLengthMismatch { .. }));
    }

    #[test]
    fn inclusion_proofs_are_validated_when_attached() {
        let l = ledger_of(6);
        let p = Ed25519Prover::generate();
        let opts = SealOptions {
            label: None,
            inclusions: vec![0, 2, 5],
        };
        let r = p.seal_with_options(&l, 0, 5, &opts).unwrap();
        assert_eq!(r.inclusions.len(), 3);

        let v = Ed25519Verifier::from_receipt(&r).unwrap();
        v.verify(&r, &l).unwrap();
    }

    #[test]
    fn forged_inclusion_proof_is_rejected() {
        let l = ledger_of(6);
        let p = Ed25519Prover::generate();
        let opts = SealOptions {
            label: None,
            inclusions: vec![3],
        };
        let mut r = p.seal_with_options(&l, 0, 5, &opts).unwrap();
        // Mutate the inclusion's self_hash to claim a different leaf.
        r.inclusions[0].self_hash_hex = hex::encode([0xCDu8; 32]);

        let v = Ed25519Verifier::from_receipt(&r).unwrap();
        let err = v.verify(&r, &l).unwrap_err();
        assert!(matches!(err, VerificationError::InclusionLeafMismatch(3)));
    }

    #[test]
    fn with_pubkey_must_match_receipt_to_verify() {
        let l = ledger_of(4);
        let p1 = Ed25519Prover::generate();
        let p2 = Ed25519Prover::generate();
        let r = p1.seal(&l, 0, 3).unwrap();
        // Verify under p2's public key (wrong identity) — signature
        // check fails.
        let v = Ed25519Verifier::with_pubkey(p2.verifying_key());
        let err = v.verify(&r, &l).unwrap_err();
        assert!(matches!(err, VerificationError::Signature(_)));
    }
}
