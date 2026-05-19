//! Audit trail for mnemonic sovereignty.
//!
//! All nine governance primitives emit structured `AuditEntry` records.
//! Entries flow to a tracing subscriber by default; production deployments
//! can swap to a file or OpenTelemetry collector via `AuditDestination`.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::ledger::{new_shared_ledger, AuditLedger, LedgerEntry, SharedLedger};

/// How to record memory governance events.
#[derive(Debug, Clone, Default)]
pub struct AuditPolicy {
    pub mode: AuditMode,
    pub destination: AuditDestination,
    pub format: AuditFormat,
    /// Optional tamper-evident hash chain over every emitted entry.
    ///
    /// `None` (the default) preserves the original emission semantics
    /// — `emit` simply ships the entry to `destination` and forgets. When
    /// `Some(ledger)`, every entry is also sealed into the [`AuditLedger`]
    /// before being shipped; downstream verification (`verify_chain`,
    /// `merkle_root`) becomes available.
    ///
    /// Set via [`Self::with_ledger`] or by directly assigning a shared
    /// ledger you want to share across multiple `AuditPolicy` clones.
    pub ledger: Option<SharedLedger>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AuditMode {
    /// Every write generates an audit entry.
    Full,
    /// Only beliefs marked `audit = "full"`.
    #[default]
    Selective,
    /// Only on contract violations.
    ViolationsOnly,
}

/// Where audit entries are written.
#[derive(Debug, Clone, Default)]
pub enum AuditDestination {
    #[default]
    Tracing,
    File { path: String },
    /// External sink (e.g. OpenTelemetry collector). TD: requires OTEL SDK.
    Otlp { endpoint: String },
}

/// Wire format for audit entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum AuditFormat {
    #[default]
    Json,
    /// OpenTelemetry-compatible structured log.
    Otel,
}

// ── AuditEntry ────────────────────────────────────────────────────────────────

/// A structured record emitted for every governance event.
///
/// Compatible with enterprise observability pipelines (JSON Lines → OTEL collector).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp_ms: u64,
    pub event_type: AuditEventType,
    pub belief_key: Option<String>,
    pub agent_id: Option<String>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuditEventType {
    BeliefWrite,
    BeliefUpdate,
    BeliefRead,
    BeliefForgotten,
    ContractViolation,
    RecoveryExecuted,
    AuthDenied,
}

impl AuditEntry {
    pub fn now(
        event_type: AuditEventType,
        belief_key: Option<String>,
        agent_id: Option<String>,
        details: serde_json::Value,
    ) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self { timestamp_ms, event_type, belief_key, agent_id, details }
    }
}

// ── Emission ──────────────────────────────────────────────────────────────────

impl AuditPolicy {
    /// Enable the tamper-evident Merkle audit ledger with a fresh, empty
    /// internal store. The returned policy owns an `Arc<Mutex<AuditLedger>>`
    /// — clones share the same underlying chain, which matches how
    /// `MnemonicSovereignty` clones flow through the runtime.
    pub fn with_ledger(mut self) -> Self {
        self.ledger = Some(new_shared_ledger());
        self
    }

    /// Enable the ledger with a caller-supplied shared store. Useful when
    /// the same ledger should accumulate entries from multiple policies
    /// (e.g. write-auth + update-auth audited into one chain).
    pub fn with_shared_ledger(mut self, ledger: SharedLedger) -> Self {
        self.ledger = Some(ledger);
        self
    }

    /// Return a handle to the underlying ledger, if one is attached. Used by
    /// callers that need to call `verify_chain()` / `merkle_root()` outside
    /// the emission hot path.
    pub fn ledger_handle(&self) -> Option<SharedLedger> {
        self.ledger.clone()
    }

    /// Emit an audit entry according to the configured destination and format.
    ///
    /// When a [`SharedLedger`] is attached, the entry is sealed into the
    /// chain *before* being dispatched — so a destination write that fails
    /// (file I/O error, tracing back-pressure) still leaves the chain
    /// intact. A poisoned ledger lock is treated as a hard error and
    /// silently skips the append; the destination dispatch still happens,
    /// matching the pre-existing best-effort semantics of `emit`.
    pub fn emit(&self, entry: &AuditEntry) {
        if let Some(ledger) = &self.ledger {
            if let Ok(mut l) = ledger.lock() {
                let _sealed: LedgerEntry = l.append(entry.clone());
            }
        }
        match &self.destination {
            AuditDestination::Tracing => {
                let json = serde_json::to_string(entry).unwrap_or_default();
                tracing::info!(
                    audit_event = %json,
                    "[epica-audit]"
                );
            }
            AuditDestination::File { path } => {
                // Append a JSON line. In production use a buffered writer / rotation.
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let line = serde_json::to_string(entry).unwrap_or_default();
                    let _ = writeln!(f, "{line}");
                }
            }
            AuditDestination::Otlp { .. } => {
                // TD: wire OTEL SDK when stabilized. Fall back to tracing for now.
                let json = serde_json::to_string(entry).unwrap_or_default();
                tracing::info!(audit_event = %json, "[epica-audit:otlp-pending]");
            }
        }
    }
}
