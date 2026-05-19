//! `epica-verify` — command-line interface for the Sprint-3 audit
//! pipeline.
//!
//! Three subcommands:
//!
//! - `keygen`: generate an Ed25519 prover keypair, write the secret
//!   bytes to a file (hex-encoded, one line), print the public key to
//!   stdout.
//! - `seal`: read a serialised [`AuditLedger`][epica_contracts::AuditLedger]
//!   from disk, sign a window of it, write the resulting
//!   [`EvidenceReceipt`][epica_zk_evidence::EvidenceReceipt] to disk.
//! - `verify`: read a ledger + a receipt, return exit code 0 + "OK" on
//!   success, non-zero + error message on failure. This is the entry
//!   point an enterprise auditor scripts against.
//!
//! Ledgers and receipts are exchanged as JSON. The CLI takes care of
//! the file I/O; the cryptographic checks live in `epica-zk-evidence`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use epica_contracts::{AuditLedger, LedgerEntry};
use epica_zk_evidence::{
    Ed25519Prover, Ed25519Verifier, EvidenceReceipt,
};

/// `epica-verify` — audit-receipt tooling for Epica.
#[derive(Debug, Parser)]
#[command(
    name = "epica-verify",
    version,
    about = "Sign and verify Epica audit ledgers (Sprint-3 commitment-based receipts).",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate a fresh Ed25519 keypair and write the secret to a file.
    Keygen {
        /// Path to write the hex-encoded secret bytes (32 bytes ⇒ 64
        /// hex chars on one line). Refuses to overwrite an existing
        /// file.
        #[arg(long)]
        secret_out: PathBuf,
    },
    /// Sign a window of a ledger and emit a JSON receipt.
    Seal {
        /// Path to the serialised ledger (JSON list of `LedgerEntry`).
        #[arg(long)]
        ledger: PathBuf,
        /// Path to the prover's hex-encoded secret bytes.
        #[arg(long)]
        secret: PathBuf,
        /// 0-based inclusive start of the window to seal.
        #[arg(long, default_value_t = 0)]
        start: u64,
        /// 0-based inclusive end of the window to seal. If omitted,
        /// the last entry of the ledger is used.
        #[arg(long)]
        end: Option<u64>,
        /// Optional free-form label embedded in the receipt metadata.
        #[arg(long)]
        label: Option<String>,
        /// Path to write the resulting receipt JSON.
        #[arg(long)]
        out: PathBuf,
    },
    /// Verify a receipt against the ledger it claims to commit to.
    Verify {
        /// Path to the serialised ledger.
        #[arg(long)]
        ledger: PathBuf,
        /// Path to the receipt JSON.
        #[arg(long)]
        receipt: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("epica-verify: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Keygen { secret_out } => keygen(&secret_out),
        Command::Seal {
            ledger,
            secret,
            start,
            end,
            label,
            out,
        } => seal_cmd(&ledger, &secret, start, end, label, &out),
        Command::Verify { ledger, receipt } => verify_cmd(&ledger, &receipt),
    }
}

// ── keygen ───────────────────────────────────────────────────────────────────

fn keygen(secret_out: &PathBuf) -> Result<(), CliError> {
    if secret_out.exists() {
        return Err(CliError::msg(format!(
            "refusing to overwrite existing secret file: {}",
            secret_out.display()
        )));
    }
    let prover = Ed25519Prover::generate();
    let secret_hex = hex::encode(prover.export_secret_bytes());
    std::fs::write(secret_out, format!("{secret_hex}\n"))
        .map_err(|e| CliError::msg(format!("write secret: {e}")))?;
    println!("public_key: {}", prover.public_key_hex());
    println!("secret written to: {}", secret_out.display());
    Ok(())
}

// ── seal ─────────────────────────────────────────────────────────────────────

fn seal_cmd(
    ledger_path: &PathBuf,
    secret_path: &PathBuf,
    start: u64,
    end: Option<u64>,
    label: Option<String>,
    out_path: &PathBuf,
) -> Result<(), CliError> {
    let ledger = read_ledger(ledger_path)?;
    if ledger.is_empty() {
        return Err(CliError::msg("ledger is empty — nothing to seal"));
    }
    let end = end.unwrap_or((ledger.len() as u64) - 1);

    let secret_hex = std::fs::read_to_string(secret_path)
        .map_err(|e| CliError::msg(format!("read secret: {e}")))?;
    let secret_bytes = hex::decode(secret_hex.trim())
        .map_err(|e| CliError::msg(format!("decode secret hex: {e}")))?;
    let prover = Ed25519Prover::from_secret_bytes(&secret_bytes)
        .map_err(|e| CliError::msg(format!("load prover: {e}")))?;

    let opts = epica_zk_evidence::prover::SealOptions {
        label,
        inclusions: Vec::new(),
    };
    let receipt = prover
        .seal_with_options(&ledger, start, end, &opts)
        .map_err(|e| CliError::msg(format!("seal: {e}")))?;

    let json = serde_json::to_string_pretty(&receipt)
        .map_err(|e| CliError::msg(format!("serialise receipt: {e}")))?;
    std::fs::write(out_path, json)
        .map_err(|e| CliError::msg(format!("write receipt: {e}")))?;

    println!("OK: sealed entries {start}..={end} of ledger ({} total)", ledger.len());
    println!("    merkle_root: {}", receipt.merkle_root_hex);
    println!("    prover_pubkey: {}", receipt.prover_pubkey_hex);
    println!("    receipt: {}", out_path.display());
    Ok(())
}

// ── verify ───────────────────────────────────────────────────────────────────

fn verify_cmd(ledger_path: &PathBuf, receipt_path: &PathBuf) -> Result<(), CliError> {
    let ledger = read_ledger(ledger_path)?;
    let receipt_json = std::fs::read_to_string(receipt_path)
        .map_err(|e| CliError::msg(format!("read receipt: {e}")))?;
    let receipt: EvidenceReceipt = serde_json::from_str(&receipt_json)
        .map_err(|e| CliError::msg(format!("parse receipt JSON: {e}")))?;

    let verifier = Ed25519Verifier::from_receipt(&receipt)
        .map_err(|e| CliError::msg(format!("build verifier: {e}")))?;
    verifier
        .verify(&receipt, &ledger)
        .map_err(|e| CliError::msg(format!("verification failed: {e}")))?;

    println!(
        "OK: receipt verifies — entries {}..={} of {} sealed by {}",
        receipt.start_seq,
        receipt.end_seq,
        receipt.ledger_len,
        &receipt.prover_pubkey_hex[..16]
    );
    if let Some(label) = &receipt.meta.label {
        println!("    label: {label}");
    }
    if !receipt.inclusions.is_empty() {
        println!("    {} per-entry inclusion proofs verified", receipt.inclusions.len());
    }
    Ok(())
}

// ── Ledger I/O ───────────────────────────────────────────────────────────────

/// Read a ledger from `path`. The on-disk format is a JSON array of
/// `LedgerEntry` — produced by callers that already serialise the
/// `AuditLedger::entries()` slice (the inner state of `AuditLedger` is
/// otherwise not directly serialisable today; see TD-P9-001).
fn read_ledger(path: &PathBuf) -> Result<AuditLedger, CliError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| CliError::msg(format!("read ledger {}: {e}", path.display())))?;
    let entries: Vec<LedgerEntryWire> = serde_json::from_str(&raw)
        .map_err(|e| CliError::msg(format!("parse ledger JSON: {e}")))?;
    rebuild_ledger(entries)
}

/// JSON wire copy of [`LedgerEntry`]. We can't directly `Deserialize`
/// `LedgerEntry` because the upstream type doesn't derive `Deserialize`
/// yet (only `Serialize` makes sense once the chain is built). The
/// CLI works around that by deserialising a parallel struct and
/// rebuilding the chain through [`AuditLedger::append`], which
/// re-validates the invariants — so a ledger that survives parsing is
/// also a ledger that survives `verify_chain`.
#[derive(serde::Deserialize)]
struct LedgerEntryWire {
    seq: u64,
    entry: epica_contracts::AuditEntry,
}

fn rebuild_ledger(entries: Vec<LedgerEntryWire>) -> Result<AuditLedger, CliError> {
    let mut ledger = AuditLedger::new();
    for (i, e) in entries.into_iter().enumerate() {
        if e.seq != i as u64 {
            return Err(CliError::msg(format!(
                "ledger entry {i} has non-monotonic seq {}",
                e.seq
            )));
        }
        ledger.append(e.entry);
    }
    Ok(ledger)
}

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug)]
struct CliError(String);

impl CliError {
    fn msg(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CliError {}

/// Public re-export of the wire struct used by tests and downstream
/// tooling that wants to serialise ledgers in the same shape this CLI
/// reads.
#[allow(dead_code)]
mod export {
    use serde::Serialize;
    /// Mirror of the CLI's internal `LedgerEntryWire` for callers that
    /// want to produce the same on-disk format from their own code.
    #[derive(Debug, Serialize)]
    pub struct LedgerEntryWire<'a> {
        pub seq: u64,
        pub entry: &'a epica_contracts::AuditEntry,
    }
}
