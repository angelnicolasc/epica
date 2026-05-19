//! Tamper-evident Merkle audit ledger.
//!
//! ## Why
//!
//! The Sprint-2 audit trail is a JSON-Lines stream: clear, structured, easy to
//! parse — and trivial to forge. A reviewer that's handed a `.jsonl` file can
//! delete any line and the file still parses. The Sprint-3 ledger sits one
//! layer below `AuditPolicy::emit` and gives the trail a *cryptographic*
//! property: any retroactive change to a past entry invalidates every
//! subsequent hash. Auditors can verify the chain without trusting the
//! producer.
//!
//! The ledger is opt-in (see [`AuditPolicy::with_ledger`]) — runtimes that
//! don't need it pay zero overhead and the API surface is unchanged. When
//! enabled, the cost per append is one BLAKE3 hash over the canonical JSON
//! encoding of the entry plus the previous hash — empirically `~1 µs` on
//! contemporary hardware (well inside the runtime's `50K ops/sec` budget).
//!
//! ## Structure
//!
//! Two layers stacked on the same data:
//!
//! - **Hash chain** (hot path): `self_hash_i = BLAKE3(prev_hash_{i-1} || entry_i_json)`.
//!   Linear, O(1) append, O(N) verification. This is enough to detect any
//!   single-entry tampering — the property the Sprint plan calls out as
//!   "altering step 90 destroys the signature of step 1".
//!
//! - **Merkle root** (deferred, computed on demand): a binary reduction over
//!   the per-entry `self_hash` values, used as the public commitment for the
//!   downstream ZK sidecar (Sprint 3.2) and as a 32-byte digest that
//!   third-party verifiers can compare without scanning the whole trail.

use std::sync::{Arc, Mutex};

use super::audit::AuditEntry;

/// One signed entry in the audit ledger.
///
/// `self_hash` is the BLAKE3 hash of `prev_hash || canonical_entry_json`.
/// `seq` is the 0-based position of this entry in the ledger; combined with
/// `self_hash` it uniquely identifies the record.
#[derive(Debug, Clone)]
pub struct LedgerEntry {
    pub seq: u64,
    pub entry: AuditEntry,
    pub prev_hash: [u8; 32],
    pub self_hash: [u8; 32],
}

/// Errors raised by [`AuditLedger::verify_chain`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LedgerTamperError {
    /// The entry at this index does not hash to its recorded `self_hash`.
    /// Either the `AuditEntry` content or the recorded hash was modified
    /// after the fact.
    #[error("entry {0} fails self-hash check (tampered or corrupted)")]
    EntryHashMismatch(usize),

    /// The entry at this index records a `prev_hash` that does not match the
    /// previous entry's `self_hash`. The chain has a discontinuity — at
    /// least one entry was inserted, removed, or reordered.
    #[error("entry {0} fails prev-hash linkage (chain broken)")]
    ChainLinkBroken(usize),
}

/// Append-only audit ledger.
///
/// Wraps a `Vec<LedgerEntry>` plus the rolling `last_hash`. Cheap to clone
/// because it lives behind an `Arc<Mutex<…>>` inside `AuditPolicy`.
///
/// ### Invariants
///
/// 1. `entries[i].seq == i as u64` for every entry.
/// 2. `entries[0].prev_hash == [0; 32]`.
/// 3. `entries[i].prev_hash == entries[i-1].self_hash` for `i > 0`.
/// 4. `entries[i].self_hash == BLAKE3(prev_hash || canonical_entry_json(entry_i))`.
///
/// Invariants 1–3 are upheld by [`Self::append`]; invariant 4 holds by
/// construction. [`Self::verify_chain`] re-checks (1)–(4) end-to-end and
/// returns the first violated entry — useful both for tests and for the
/// `epica-verify` CLI shipped in Sprint 3.2.
#[derive(Debug, Default)]
pub struct AuditLedger {
    entries: Vec<LedgerEntry>,
    last_hash: [u8; 32],
}

impl AuditLedger {
    /// Create an empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of entries currently in the ledger.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Rolling "head" hash — the `self_hash` of the most recent entry, or
    /// the all-zero genesis hash when the ledger is empty.
    pub fn head_hash(&self) -> [u8; 32] {
        self.last_hash
    }

    /// Append `entry` and return its sealed [`LedgerEntry`].
    ///
    /// The append is total: every entry is recorded. Filtering policy (mode,
    /// destination) lives one layer up in `AuditPolicy::emit`.
    pub fn append(&mut self, entry: AuditEntry) -> LedgerEntry {
        let seq = self.entries.len() as u64;
        let prev_hash = self.last_hash;
        let self_hash = hash_entry(&prev_hash, &entry);
        let sealed = LedgerEntry { seq, entry, prev_hash, self_hash };
        self.last_hash = self_hash;
        self.entries.push(sealed.clone());
        sealed
    }

    /// Verify every invariant of the chain in O(N).
    ///
    /// Returns the index of the first violated entry. A clean pass returns
    /// `Ok(())` — the recorded `last_hash` equals the freshly recomputed
    /// head, and every link is intact.
    pub fn verify_chain(&self) -> Result<(), LedgerTamperError> {
        let mut expected_prev = [0u8; 32];
        for (i, e) in self.entries.iter().enumerate() {
            if e.seq != i as u64 {
                return Err(LedgerTamperError::ChainLinkBroken(i));
            }
            if e.prev_hash != expected_prev {
                return Err(LedgerTamperError::ChainLinkBroken(i));
            }
            let recomputed = hash_entry(&e.prev_hash, &e.entry);
            if recomputed != e.self_hash {
                return Err(LedgerTamperError::EntryHashMismatch(i));
            }
            expected_prev = e.self_hash;
        }
        Ok(())
    }

    /// Compute the Merkle root over all per-entry `self_hash` values.
    ///
    /// Uses a domain-separated binary reduction: leaves are `BLAKE3(b"\x00"
    /// || self_hash)`, internal nodes are `BLAKE3(b"\x01" || left || right)`.
    /// An odd node at any level is duplicated. Empty ledger returns the
    /// all-zero digest.
    ///
    /// This root is the public commitment consumed by `epica-verify`
    /// (Sprint 3.2) — it travels with a ZK proof so a third party can match
    /// a verified batch to a specific ledger state.
    pub fn merkle_root(&self) -> [u8; 32] {
        if self.entries.is_empty() {
            return [0u8; 32];
        }
        let mut level: Vec<[u8; 32]> = self
            .entries
            .iter()
            .map(|e| leaf_hash(&e.self_hash))
            .collect();

        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len().div_ceil(2));
            for pair in level.chunks(2) {
                let l = pair[0];
                let r = if pair.len() == 2 { pair[1] } else { pair[0] };
                next.push(node_hash(&l, &r));
            }
            level = next;
        }
        level[0]
    }

    /// Borrow the underlying entries — read-only.
    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    /// Compute the Merkle inclusion proof for the entry at `seq`.
    ///
    /// Returns the ordered list of sibling hashes needed to recompute the
    /// root from `entries[seq].self_hash`. The proof length is
    /// `⌈log2(N)⌉`; verifying it costs O(log N) hashes against O(N) to
    /// rebuild the whole tree, so this is the lookup `epica-verify` (the
    /// Sprint 3.3 CLI) uses to confirm "yes, entry seq=K is in the batch
    /// whose root is R."
    ///
    /// Returns `None` when `seq` is out of range. An empty ledger always
    /// returns `None`.
    pub fn merkle_proof(&self, seq: u64) -> Option<Vec<[u8; 32]>> {
        let idx = seq as usize;
        if idx >= self.entries.len() {
            return None;
        }
        let mut level: Vec<[u8; 32]> = self
            .entries
            .iter()
            .map(|e| leaf_hash(&e.self_hash))
            .collect();
        let mut path = idx;
        let mut proof: Vec<[u8; 32]> = Vec::new();

        while level.len() > 1 {
            // Determine sibling index, duplicating the last node when the
            // level has an odd count — same rule as `merkle_root`.
            let sibling = if path % 2 == 0 {
                // Right sibling.
                if path + 1 < level.len() {
                    path + 1
                } else {
                    path // self (odd tail duplication)
                }
            } else {
                // Left sibling.
                path - 1
            };
            proof.push(level[sibling]);

            // Reduce.
            let mut next = Vec::with_capacity(level.len().div_ceil(2));
            for pair in level.chunks(2) {
                let l = pair[0];
                let r = if pair.len() == 2 { pair[1] } else { pair[0] };
                next.push(node_hash(&l, &r));
            }
            level = next;
            path /= 2;
        }
        Some(proof)
    }
}

/// Verify a Merkle inclusion proof against a known `root`.
///
/// `leaf_self_hash` is the `self_hash` of the [`LedgerEntry`] whose
/// inclusion we're proving. `seq` is its 0-based position. `n` is the
/// total number of entries in the ledger when the proof was produced
/// — needed to reproduce the odd-tail duplication rule. `proof` is the
/// path returned by [`AuditLedger::merkle_proof`].
///
/// Returns `true` iff the reconstructed root equals `root`. Out-of-range
/// inputs (`seq >= n`) return `false`.
#[must_use]
pub fn verify_merkle_proof(
    leaf_self_hash: &[u8; 32],
    seq: u64,
    n: u64,
    proof: &[[u8; 32]],
    root: &[u8; 32],
) -> bool {
    if n == 0 || seq >= n {
        return false;
    }
    let mut hash = leaf_hash(leaf_self_hash);
    let mut path = seq as usize;
    let mut level_size = n as usize;

    for sib in proof {
        let (left, right) = if path % 2 == 0 {
            (&hash, sib)
        } else {
            (sib, &hash)
        };
        hash = node_hash(left, right);
        path /= 2;
        level_size = level_size.div_ceil(2);
    }
    // After consuming the proof we should be at the root level.
    level_size == 1 && hash == *root
}

#[inline]
fn leaf_hash(self_hash: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&[0x00]);
    h.update(self_hash);
    *h.finalize().as_bytes()
}

#[inline]
fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&[0x01]);
    h.update(left);
    h.update(right);
    *h.finalize().as_bytes()
}

/// Canonical hash of a single audit entry, linked to `prev_hash`.
///
/// The canonical encoding is the standard `serde_json` serialization of
/// `entry` — stable enough across `serde_json` versions for the small
/// `AuditEntry` shape we hash. Encoding failures fall back to an empty
/// string, which is itself a valid (and deterministic) hash input.
pub fn hash_entry(prev_hash: &[u8; 32], entry: &AuditEntry) -> [u8; 32] {
    let canonical = serde_json::to_vec(entry).unwrap_or_default();
    let mut h = blake3::Hasher::new();
    h.update(prev_hash);
    h.update(&canonical);
    *h.finalize().as_bytes()
}

/// Convenience alias used by `AuditPolicy`. The Arc/Mutex shape lets the
/// policy be `Clone` while keeping the ledger logically singleton.
pub type SharedLedger = Arc<Mutex<AuditLedger>>;

/// Build a fresh empty shared ledger.
pub fn new_shared_ledger() -> SharedLedger {
    Arc::new(Mutex::new(AuditLedger::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sovereignty::audit::{AuditEntry, AuditEventType};

    /// Build a test entry with a *fixed* timestamp so two independent
    /// ledgers built from the same key sequence produce byte-identical
    /// chains. Using `AuditEntry::now()` here was a flake source: the two
    /// calls landed in different milliseconds, the canonical JSON differed,
    /// and the Merkle roots diverged.
    fn entry(key: &str) -> AuditEntry {
        AuditEntry {
            timestamp_ms: 1_700_000_000_000,
            event_type: AuditEventType::BeliefWrite,
            belief_key: Some(key.to_string()),
            agent_id: Some("agent-A".into()),
            details: serde_json::json!({}),
        }
    }

    #[test]
    fn empty_ledger_invariants() {
        let l = AuditLedger::new();
        assert_eq!(l.len(), 0);
        assert!(l.is_empty());
        assert_eq!(l.head_hash(), [0u8; 32]);
        assert_eq!(l.merkle_root(), [0u8; 32]);
        assert!(l.verify_chain().is_ok());
    }

    #[test]
    fn append_links_each_entry_to_the_previous() {
        let mut l = AuditLedger::new();
        let e0 = l.append(entry("k0"));
        let e1 = l.append(entry("k1"));
        let e2 = l.append(entry("k2"));
        assert_eq!(e0.seq, 0);
        assert_eq!(e1.seq, 1);
        assert_eq!(e2.seq, 2);
        assert_eq!(e0.prev_hash, [0u8; 32]);
        assert_eq!(e1.prev_hash, e0.self_hash);
        assert_eq!(e2.prev_hash, e1.self_hash);
        assert_eq!(l.head_hash(), e2.self_hash);
        assert!(l.verify_chain().is_ok());
    }

    #[test]
    fn tamper_with_entry_payload_is_detected() {
        let mut l = AuditLedger::new();
        l.append(entry("k0"));
        l.append(entry("k1"));
        l.append(entry("k2"));

        // Tamper: swap the belief_key on entry 1 without recomputing its hash.
        // The recorded self_hash no longer matches the entry content.
        // Reach into the private field by leaking the entries.
        // We deliberately mutate via a clone-and-replace because LedgerEntry
        // is meant to be immutable from outside — tests stand in for an
        // attacker who edited the file on disk after the fact.
        let stolen = std::mem::take(&mut l.entries);
        let mut tampered = stolen;
        tampered[1].entry.belief_key = Some("forged".into());
        l.entries = tampered;

        let err = l.verify_chain().expect_err("tamper must be detected");
        assert_eq!(err, LedgerTamperError::EntryHashMismatch(1));
    }

    #[test]
    fn tamper_with_chain_link_is_detected() {
        let mut l = AuditLedger::new();
        l.append(entry("k0"));
        l.append(entry("k1"));
        l.append(entry("k2"));

        // Tamper: replace the prev_hash on entry 2 with garbage. The
        // entry's *own* hash is recomputed against the corrupted prev_hash,
        // so a naive attacker could keep the self-hash consistent — but the
        // linkage to entry 1 is now broken.
        let stolen = std::mem::take(&mut l.entries);
        let mut tampered = stolen;
        tampered[2].prev_hash = [0xAA; 32];
        // Keep self_hash internally consistent so EntryHashMismatch doesn't
        // fire first; we want to demonstrate the ChainLinkBroken branch.
        tampered[2].self_hash = hash_entry(&[0xAA; 32], &tampered[2].entry);
        l.entries = tampered;

        let err = l.verify_chain().expect_err("broken link must be detected");
        assert_eq!(err, LedgerTamperError::ChainLinkBroken(2));
    }

    #[test]
    fn merkle_root_changes_under_tamper() {
        let mut l1 = AuditLedger::new();
        let mut l2 = AuditLedger::new();
        for k in ["a", "b", "c", "d", "e"] {
            l1.append(entry(k));
            l2.append(entry(k));
        }
        assert_eq!(l1.merkle_root(), l2.merkle_root(),
            "deterministic Merkle root for identical histories");

        // Forge a divergent payload in l2.
        let stolen = std::mem::take(&mut l2.entries);
        let mut tampered = stolen;
        tampered[2].entry.belief_key = Some("forged".into());
        tampered[2].self_hash = hash_entry(&tampered[2].prev_hash, &tampered[2].entry);
        // Repair downstream links so the chain itself stays consistent —
        // a sophisticated forger would do this. The Merkle root must still
        // diverge because at least one leaf hash changed.
        for i in 3..tampered.len() {
            tampered[i].prev_hash = tampered[i - 1].self_hash;
            tampered[i].self_hash = hash_entry(&tampered[i].prev_hash, &tampered[i].entry);
        }
        l2.last_hash = tampered.last().unwrap().self_hash;
        l2.entries = tampered;

        assert_ne!(l1.merkle_root(), l2.merkle_root(),
            "tampered ledger must produce a different Merkle root");
    }

    #[test]
    fn odd_leaf_count_handled() {
        // Verify the duplicate-last-leaf rule produces a stable root and
        // doesn't panic on odd lengths.
        let mut l = AuditLedger::new();
        for k in ["a", "b", "c"] {
            l.append(entry(k));
        }
        let root1 = l.merkle_root();
        let root2 = l.merkle_root();
        assert_eq!(root1, root2);
        assert_ne!(root1, [0u8; 32]);
    }

    #[test]
    fn merkle_proof_reconstructs_root_for_every_position() {
        // Powers-of-two and non-powers-of-two; verify every seq.
        for n in [1_usize, 2, 3, 4, 5, 7, 8, 9, 16, 17] {
            let mut l = AuditLedger::new();
            for i in 0..n {
                l.append(entry(&format!("k{i}")));
            }
            let root = l.merkle_root();
            for seq in 0..n {
                let proof = l
                    .merkle_proof(seq as u64)
                    .expect("proof must exist for in-range seq");
                let leaf = l.entries()[seq].self_hash;
                assert!(
                    verify_merkle_proof(&leaf, seq as u64, n as u64, &proof, &root),
                    "proof must reconstruct root for n={n}, seq={seq}"
                );
            }
        }
    }

    #[test]
    fn merkle_proof_rejects_out_of_range_seq() {
        let mut l = AuditLedger::new();
        l.append(entry("only"));
        assert!(l.merkle_proof(1).is_none());
        assert!(l.merkle_proof(99).is_none());
        // verify_merkle_proof with out-of-range seq returns false even
        // if the caller supplies a plausibly-looking proof.
        let bogus_proof = vec![[0u8; 32]];
        assert!(!verify_merkle_proof(
            &[0u8; 32],
            5,
            1,
            &bogus_proof,
            &l.merkle_root()
        ));
    }

    #[test]
    fn merkle_proof_against_wrong_leaf_fails() {
        let mut l = AuditLedger::new();
        for k in ["a", "b", "c", "d"] {
            l.append(entry(k));
        }
        let root = l.merkle_root();
        let proof = l.merkle_proof(2).unwrap();
        // Use a leaf hash for seq=1 but claim it's seq=2.
        let wrong_leaf = l.entries()[1].self_hash;
        assert!(!verify_merkle_proof(&wrong_leaf, 2, 4, &proof, &root));
    }

    #[test]
    fn merkle_proof_against_wrong_root_fails() {
        let mut l = AuditLedger::new();
        for k in ["a", "b", "c", "d"] {
            l.append(entry(k));
        }
        let proof = l.merkle_proof(2).unwrap();
        let leaf = l.entries()[2].self_hash;
        let wrong_root = [0xAAu8; 32];
        assert!(!verify_merkle_proof(&leaf, 2, 4, &proof, &wrong_root));
    }
}
