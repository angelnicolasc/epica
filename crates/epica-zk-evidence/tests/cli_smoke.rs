//! End-to-end smoke tests for the `epica-verify` binary.
//!
//! The binary is built by `cargo test` via `env!("CARGO_BIN_EXE_*")`, so
//! these tests exercise the actual CLI surface — argv parsing, file I/O,
//! exit codes — not just the library APIs the binary calls.

use std::path::PathBuf;
use std::process::Command;

use epica_contracts::{AuditEntry, AuditEventType, AuditLedger};
use serde::Serialize;

fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_epica-verify"))
}

/// JSON wire shape the CLI reads. Mirror of `LedgerEntryWire` inside
/// the binary — tests need a `Serialize` view; the binary only needs a
/// `Deserialize` view.
#[derive(Serialize)]
struct LedgerEntryWire<'a> {
    seq: u64,
    entry: &'a AuditEntry,
}

fn entry(key: &str) -> AuditEntry {
    AuditEntry {
        timestamp_ms: 1_700_000_000_000,
        event_type: AuditEventType::BeliefWrite,
        belief_key: Some(key.to_string()),
        agent_id: Some("test-agent".into()),
        details: serde_json::json!({}),
    }
}

fn write_ledger_json(path: &PathBuf, entries: &[AuditEntry]) {
    let mut ledger = AuditLedger::new();
    for e in entries {
        ledger.append(e.clone());
    }
    let wire: Vec<LedgerEntryWire> = ledger
        .entries()
        .iter()
        .map(|e| LedgerEntryWire {
            seq: e.seq,
            entry: &e.entry,
        })
        .collect();
    let json = serde_json::to_string_pretty(&wire).unwrap();
    std::fs::write(path, json).unwrap();
}

#[test]
fn keygen_seal_verify_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let secret = tmp.path().join("secret.hex");
    let ledger = tmp.path().join("ledger.json");
    let receipt = tmp.path().join("receipt.json");

    write_ledger_json(
        &ledger,
        &[entry("k0"), entry("k1"), entry("k2"), entry("k3")],
    );

    // keygen
    let out = Command::new(binary_path())
        .args(["keygen", "--secret-out"])
        .arg(&secret)
        .output()
        .unwrap();
    assert!(out.status.success(), "keygen failed: {out:?}");
    assert!(secret.exists());

    // seal
    let out = Command::new(binary_path())
        .args(["seal", "--ledger"])
        .arg(&ledger)
        .arg("--secret")
        .arg(&secret)
        .args(["--start", "0", "--end", "3", "--label", "smoke"])
        .arg("--out")
        .arg(&receipt)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "seal failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(receipt.exists());

    // verify
    let out = Command::new(binary_path())
        .args(["verify", "--ledger"])
        .arg(&ledger)
        .arg("--receipt")
        .arg(&receipt)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "verify failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("OK:"));
    assert!(stdout.contains("smoke"), "label must surface: {stdout}");
}

#[test]
fn verify_fails_on_tampered_ledger() {
    let tmp = tempfile::tempdir().unwrap();
    let secret = tmp.path().join("secret.hex");
    let ledger = tmp.path().join("ledger.json");
    let bad_ledger = tmp.path().join("tampered.json");
    let receipt = tmp.path().join("receipt.json");

    let originals = vec![entry("a"), entry("b"), entry("c")];
    write_ledger_json(&ledger, &originals);

    // Tamper after sealing: same N entries, but entry[1] has a forged key.
    let mut tampered = originals.clone();
    tampered[1].belief_key = Some("forged".into());
    write_ledger_json(&bad_ledger, &tampered);

    Command::new(binary_path())
        .args(["keygen", "--secret-out"])
        .arg(&secret)
        .status()
        .unwrap();
    Command::new(binary_path())
        .args(["seal", "--ledger"])
        .arg(&ledger)
        .arg("--secret")
        .arg(&secret)
        .arg("--out")
        .arg(&receipt)
        .status()
        .unwrap();

    let out = Command::new(binary_path())
        .args(["verify", "--ledger"])
        .arg(&bad_ledger)
        .arg("--receipt")
        .arg(&receipt)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "tampered verify must fail; stdout was: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("merkle root mismatch") || stderr.contains("signature"),
        "expected merkle/signature error, got: {stderr}"
    );
}

#[test]
fn keygen_refuses_to_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let secret = tmp.path().join("secret.hex");
    std::fs::write(&secret, "preexisting").unwrap();

    let out = Command::new(binary_path())
        .args(["keygen", "--secret-out"])
        .arg(&secret)
        .output()
        .unwrap();
    assert!(!out.status.success(), "must refuse overwrite");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("refusing to overwrite"),
        "expected overwrite-protection error, got: {stderr}"
    );
}

#[test]
fn seal_defaults_end_to_last_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let secret = tmp.path().join("secret.hex");
    let ledger = tmp.path().join("ledger.json");
    let receipt = tmp.path().join("receipt.json");

    write_ledger_json(&ledger, &[entry("a"), entry("b"), entry("c")]);
    Command::new(binary_path())
        .args(["keygen", "--secret-out"])
        .arg(&secret)
        .status()
        .unwrap();

    // No --end supplied. Should default to ledger.len() - 1 = 2.
    let out = Command::new(binary_path())
        .args(["seal", "--ledger"])
        .arg(&ledger)
        .arg("--secret")
        .arg(&secret)
        .arg("--out")
        .arg(&receipt)
        .output()
        .unwrap();
    assert!(out.status.success(), "seal w/o --end failed: {out:?}");
    let json = std::fs::read_to_string(&receipt).unwrap();
    let r: epica_zk_evidence::EvidenceReceipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.start_seq, 0);
    assert_eq!(r.end_seq, 2);
    assert_eq!(r.ledger_len, 3);
}
