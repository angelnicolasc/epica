//! Skeleton module — placeholder for the eventual RISC Zero zkVM proof
//! backend.
//!
//! ## Status
//!
//! Empty by design. Gated behind the `risc0` Cargo feature so the
//! ledger / Ed25519 path can ship today without dragging the RISC-V
//! toolchain into every CI run.
//!
//! ## What lands here
//!
//! When this module is wired up, it will expose:
//!
//! - `Risc0Prover`: an additional impl of the same "seal" surface that
//!   [`crate::Ed25519Prover`] exposes, but producing a STARK proof
//!   (instead of just an Ed25519 signature) that attests "the prover
//!   knew a sequence of audit entries whose recomputed Merkle root
//!   equals the one in the receipt, AND each entry corresponds to a
//!   valid AGM K\*1–K\*6 transition." The guest program runs in the
//!   RISC Zero zkVM.
//!
//! - `Risc0Verifier`: extends [`crate::Ed25519Verifier`] with an
//!   additional layer that verifies the STARK proof. Cost on the
//!   verifier side: ~10ms for a 1024-entry batch (RISC Zero's published
//!   numbers); production cost on the prover side is the limiting
//!   factor (~1s per batch).
//!
//! The receipt wire format already reserves enough room for the proof
//! bytes — they'll go in a new optional field with a `serde(default)`
//! attribute so receipts produced today (Ed25519-only) keep verifying
//! against tomorrow's binary.
//!
//! ## What blocks shipping it
//!
//! 1. `cargo install cargo-risczero` plus the `riscv32im-risc0-zkvm-elf`
//!    target must be available on every CI runner that compiles the
//!    workspace with `--features risc0`. This is a ~5-minute provision
//!    step per runner and a 200 MB toolchain download.
//! 2. The guest program needs a stable Rust re-implementation of the
//!    AGM postulate checks (`K*1`–`K*6`) that fits the zkVM's `no_std`
//!    constraints. Today those checks live inside `epica-core`'s
//!    `revision::postulates` module and depend on `petgraph` — porting
//!    requires either lifting the deps into a `no_std`-friendly subset
//!    or restating the postulate checks against the audit entry types
//!    directly.
//!
//! Both are tractable but not Sprint-3 work.

#![cfg(feature = "risc0")]

/// Compile-time marker: this module is included in the build with
/// `--features risc0`. Once the real backend lands, this constant is
/// the only thing here that *stays* as-is.
pub const RISC0_BACKEND_AVAILABLE: bool = false;
