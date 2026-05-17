//! # epica-runtime
//!
//! `BeliefRuntime`: async wrapper around `BeliefQuad` providing dual-process
//! uncertainty containment (System 1 + System 2), behavioral contract enforcement,
//! multicriteria belief retrieval, and session-level Trajectory-ECE metrics.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
//! use epica_runtime::BeliefRuntime;
//!
//! # #[tokio::main]
//! # async fn main() {
//! let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0);
//!
//! let node = BeliefNode::new(
//!     "user_intent",
//!     BeliefValue::Asserted("refactor auth module".into()),
//!     Provenance::UserStatement { turn: 0 },
//!     0.9,
//! );
//! let id = rt.insert_belief(node).await;
//! let result = rt.update_belief(
//!     id,
//!     BeliefValue::Asserted("refactor auth module".into()),
//!     Provenance::UserStatement { turn: 1 },
//!     0.85,
//! ).await.unwrap();
//!
//! rt.finalize_session().await;
//! let report = rt.session_report().await;
//! println!("T-ECE: {:?}", report.tece);
//! # }
//! ```

#![warn(missing_docs)]

pub mod contract_engine;
pub mod error;
pub mod governance_tracker;
pub mod history;
pub mod prospective;
pub mod retrieval;
pub mod runtime;
pub mod session;
pub mod system2;

pub use contract_engine::{ContractEngine, RecoveryOutcome, ViolationLog};
pub use prospective::{
    EmbedError, Embedder, HashEmbedder,
    ProspectiveClient, ProspectiveClientError, RawProspectiveScenario,
};
pub use error::{RuntimeError, RuntimeUpdateResult};
pub use governance_tracker::GovernanceTracker;
pub use history::{ConfidenceHistory, ConfidenceRecord};
pub use retrieval::{compute_score, ScoredBelief};
pub use runtime::BeliefRuntime;
pub use session::{SessionReport, TECE_TARGET};
pub use system2::{DiagnosticSignal, LlmClientError, System2Result, TokenBucket};

#[cfg(feature = "system2")]
pub use system2::LlmClient;
