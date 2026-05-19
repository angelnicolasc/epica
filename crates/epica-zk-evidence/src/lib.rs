//! # epica-zk-evidence
//!
//! Third-party-verifiable audit receipts for Epica's
//! [`AuditLedger`][epica_contracts::AuditLedger].
//!
//! ## What this crate ships *today*
//!
//! - [`EvidenceReceipt`]: the wire format that proves a specific window
//!   `[start_seq, end_seq]` of an audit ledger was sealed by a known
//!   prover. Carries the Merkle root, the batch range, the prover's
//!   public key, and a detached Ed25519 signature over the canonical
//!   binding `BLAKE3(merkle_root || start_seq || end_seq || prover_pubkey)`.
//!
//! - [`Ed25519Prover`] / [`Ed25519Verifier`]: sign and verify receipts.
//!   Together with the per-entry [`merkle_proof`][epica_contracts::AuditLedger::merkle_proof]
//!   already on the ledger, this gives every auditable property the
//!   Sprint-3 plan asked for:
//!     - **Tamper evidence**: altering any sealed entry invalidates the
//!       Merkle root → the signature fails to verify against the
//!       reconstructed root.
//!     - **Non-repudiation**: only the holder of the prover's secret key
//!       could have produced a signature that verifies under the public
//!       key embedded in the receipt.
//!     - **Third-party verifiable**: anyone with the receipt's public
//!       key and the ledger entries can run the [`epica-verify`][1] CLI
//!       offline, no network round-trip required.
//!
//! - [`epica-verify`][1]: a thin CLI wrapper around this crate. Given a
//!   JSON-serialised ledger and a receipt, prints "OK" plus the batch
//!   range or "FAIL" plus the reason.
//!
//! [1]: https://github.com/angelnicolasc/epica
//!
//! ## What this crate does *not* ship today
//!
//! The Sprint-3 plan's stretch goal was a Zero-Knowledge proof, produced
//! by a RISC Zero zkVM, that *additionally* attested "this Merkle root
//! corresponds to N audit entries each of which describes a valid AGM
//! K\*1–K\*6 transition." Two reasons that's deferred:
//!
//! 1. **Toolchain cost**: a RISC Zero guest program requires the
//!    `riscv32im-risc0-zkvm-elf` target + the `cargo risczero` extension.
//!    Setting that up on every CI runner is a multi-hour delta we don't
//!    want to pay before Sprint 4 has produced something worth proving.
//! 2. **Value gap**: the *primary* property an auditor wants — "this
//!    ledger snapshot was sealed by the holder of public key X, and any
//!    edit invalidates the seal" — does **not** require ZK. Ed25519 over
//!    a Merkle commitment delivers it. ZK only adds the additional
//!    property "the prover knew a valid AGM trace without revealing
//!    payloads," which is a privacy concern, not a correctness one.
//!
//! The crate exposes a `risc0` Cargo feature whose body today is a
//! documented `todo!()` skeleton (see [`zk_skeleton`]). When the
//! toolchain is provisioned, that module is the only place new code
//! lands — public API and CLI stay stable.
//!
//! ## Quick start
//!
//! ```rust
//! use epica_contracts::{AuditEntry, AuditEventType, AuditLedger};
//! use epica_zk_evidence::{Ed25519Prover, Ed25519Verifier};
//!
//! // Producer side.
//! let mut ledger = AuditLedger::new();
//! for k in ["a", "b", "c"] {
//!     ledger.append(AuditEntry::now(
//!         AuditEventType::BeliefWrite,
//!         Some(k.into()),
//!         Some("agent-A".into()),
//!         serde_json::json!({}),
//!     ));
//! }
//!
//! let prover = Ed25519Prover::generate();
//! let receipt = prover.seal(&ledger, 0, 2).expect("seal succeeds");
//!
//! // Auditor side — only needs the receipt + the public ledger entries.
//! let verifier = Ed25519Verifier::from_receipt(&receipt).expect("decode pubkey");
//! verifier.verify(&receipt, &ledger).expect("verification");
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod prover;
pub mod receipt;
pub mod verifier;

#[cfg(feature = "risc0")]
pub mod zk_skeleton;

pub use prover::{Ed25519Prover, ProverError};
pub use receipt::{
    receipt_binding, EvidenceReceipt, EvidenceReceiptError, MerkleInclusionProof, ReceiptMetadata,
};
pub use verifier::{Ed25519Verifier, VerificationError};
