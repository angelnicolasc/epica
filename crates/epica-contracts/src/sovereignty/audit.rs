//! Audit trail for mnemonic sovereignty.
//!
//! All nine governance primitives emit structured `AuditEntry` records.
//! Entries flow to a tracing subscriber by default; production deployments
//! can swap to a file or OpenTelemetry collector via `AuditDestination`.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// How to record memory governance events.
#[derive(Debug, Clone, Default)]
pub struct AuditPolicy {
    pub mode: AuditMode,
    pub destination: AuditDestination,
    pub format: AuditFormat,
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
#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub timestamp_ms: u64,
    pub event_type: AuditEventType,
    pub belief_key: Option<String>,
    pub agent_id: Option<String>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
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
    /// Emit an audit entry according to the configured destination and format.
    pub fn emit(&self, entry: &AuditEntry) {
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
