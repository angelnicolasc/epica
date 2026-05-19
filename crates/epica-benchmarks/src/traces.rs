//! Deterministic, seedable synthetic trajectory generators.
//!
//! Two suites today, both shaped after their real-world counterparts:
//!
//! - [`Suite::AlfworldLike`] — multi-step goal trajectories. Each trace
//!   represents an agent pursuing a household task ("find apple, place
//!   in fridge"). The trace inserts a top-level goal, then a sequence
//!   of intermediate observations that progressively narrow the
//!   belief state, occasionally producing a contradiction the runtime
//!   must resolve via AGM revision. The terminal step marks the
//!   subset of inserted beliefs that turned out to be correct so
//!   `T-ECE` is computable.
//!
//! - [`Suite::WebshopLike`] — search-then-filter trajectories. Each
//!   trace inserts a user intent (Asserted, high confidence), then a
//!   sequence of candidate products as Inferred beliefs, then filter
//!   refinements that contradict prior candidates. Paraphrases of the
//!   intent appear at random — `Suite::WebshopLike` is specifically
//!   tuned to exercise the K\*6 semantic-equivalence path added in
//!   Sprint 1.
//!
//! Generation is **deterministic**: the same `(suite, trajectory_id)`
//! pair always produces the same trace. We do not pull a serious RNG
//! dependency in — a small `SplitMix64` derived from the trajectory
//! id is enough for the patterns we care about and keeps the bench
//! itself a single-file affair.

use serde::{Deserialize, Serialize};

/// Which canonical benchmark shape to generate.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Suite {
    /// Multi-step goal pursuit, ALFWorld shape.
    AlfworldLike,
    /// Search-then-filter, WebShop shape.
    WebshopLike,
}

impl Suite {
    /// Slug used for filenames, CLI args, and CSV columns.
    pub fn slug(self) -> &'static str {
        match self {
            Self::AlfworldLike => "alfworld_like",
            Self::WebshopLike => "webshop_like",
        }
    }
}

impl std::fmt::Display for Suite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.slug())
    }
}

impl std::str::FromStr for Suite {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "alfworld" | "alfworld_like" | "alfworld-like" => Ok(Self::AlfworldLike),
            "webshop" | "webshop_like" | "webshop-like" => Ok(Self::WebshopLike),
            other => Err(format!("unknown suite: {other}")),
        }
    }
}

/// One step of a synthetic trajectory.
///
/// Every step records (a) the operation the harness should perform on
/// the runtime, (b) a ground-truth `correct` flag the metric layer
/// uses when computing T-ECE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraceStep {
    /// Insert a new belief.
    Insert {
        /// Belief key.
        key: String,
        /// Asserted value.
        value: String,
        /// Confidence reported by the synthetic agent.
        confidence: f32,
        /// Ground-truth correctness — `true` if this confidence
        /// reflected reality, `false` otherwise. Used to drive
        /// T-ECE computation.
        correct: bool,
    },
    /// Revise an existing belief.
    Update {
        /// Key of the belief to update.
        key: String,
        /// New asserted value.
        value: String,
        /// New confidence.
        confidence: f32,
        /// Ground-truth correctness of the new value.
        correct: bool,
    },
}

impl TraceStep {
    /// The belief key this step operates on.
    pub fn key(&self) -> &str {
        match self {
            Self::Insert { key, .. } | Self::Update { key, .. } => key,
        }
    }

    /// Ground-truth correctness of the value this step records.
    pub fn correct(&self) -> bool {
        match self {
            Self::Insert { correct, .. } | Self::Update { correct, .. } => *correct,
        }
    }
}

/// A single deterministic trajectory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    /// Which suite this trace belongs to.
    pub suite: Suite,
    /// 0-based trajectory id; combined with `suite` it deterministically
    /// regenerates the same trace.
    pub trajectory_id: u32,
    /// Ordered steps.
    pub steps: Vec<TraceStep>,
}

impl Trace {
    /// Build the canonical trace for `(suite, trajectory_id)`.
    pub fn generate(suite: Suite, trajectory_id: u32) -> Self {
        let steps = match suite {
            Suite::AlfworldLike => alfworld_like(trajectory_id),
            Suite::WebshopLike => webshop_like(trajectory_id),
        };
        Self { suite, trajectory_id, steps }
    }
}

/// `SplitMix64`-style deterministic RNG. 100 LOC of math beats pulling
/// `rand` into a workspace-internal crate. We only need: a `u64` per
/// call, well-distributed bits across calls within a trajectory, and
/// reproducibility from the seed.
#[derive(Debug, Clone, Copy)]
struct SplitMix(u64);

impl SplitMix {
    fn seed_from(suite_tag: u64, trajectory_id: u32) -> Self {
        // Mix the suite tag into the high half so trajectory_id=0 in
        // different suites doesn't produce identical traces.
        Self((suite_tag << 32) ^ trajectory_id as u64 ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn next_f32(&mut self) -> f32 {
        // Top 24 bits → [0, 1).
        let n = (self.next_u64() >> 40) as u32;
        (n as f32) / (1u32 << 24) as f32
    }
    /// Roll true with `p` probability.
    fn bernoulli(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }
    fn pick<'a, T>(&mut self, options: &'a [T]) -> &'a T {
        let i = (self.next_u64() as usize) % options.len();
        &options[i]
    }
}

const ALFWORLD_TAG: u64 = 0xA1F4_0E0D;
const WEBSHOP_TAG: u64 = 0x4E85_4039;

/// ALFWorld-like: 8–14 steps. One Asserted goal, a sequence of Inferred
/// observations (probe results) where ~70% are correct, ~30% wrong.
/// One mid-trajectory contradiction forces an AGM revision.
fn alfworld_like(trajectory_id: u32) -> Vec<TraceStep> {
    let mut rng = SplitMix::seed_from(ALFWORLD_TAG, trajectory_id);
    let objects = ["apple", "book", "key", "remote", "vase", "knife", "candle"];
    let containers = ["fridge", "shelf", "drawer", "table", "cabinet"];
    let object = rng.pick(&objects);
    let container = rng.pick(&containers);

    let mut steps = Vec::new();

    // 1. Top-level goal — high-confidence asserted intent. Always
    //    "correct" in the sense that this is the user's stated goal.
    steps.push(TraceStep::Insert {
        key: "goal".into(),
        value: format!("place {object} in {container}"),
        confidence: 0.95,
        correct: true,
    });

    // 2. Probe results — 5..9 of them. Each one is the agent looking
    //    in a location and reporting whether the object is there.
    let probes = 5 + (rng.next_u64() % 5) as usize;
    let true_location = rng.pick(&containers);
    let mut probe_idx = 0;
    for i in 0..probes {
        let location = rng.pick(&containers);
        let found = location == true_location;
        // Agent is right ~70% of the time; otherwise reports the
        // wrong thing with mid confidence.
        let agent_correct = rng.bernoulli(0.70);
        let conf = if agent_correct {
            if found { 0.85 + rng.next_f32() * 0.10 } else { 0.10 + rng.next_f32() * 0.15 }
        } else if found {
            // Reports "not here" when actually there.
            0.20 + rng.next_f32() * 0.15
        } else {
            // Reports "here" when actually not.
            0.70 + rng.next_f32() * 0.15
        };
        let reflects_truth = (conf > 0.5) == found;
        steps.push(TraceStep::Insert {
            key: format!("probe_{i}"),
            value: format!("at {location}: {}", if conf > 0.5 { "found" } else { "absent" }),
            confidence: conf,
            correct: reflects_truth,
        });
        probe_idx = i;
    }

    // 3. One mid-trajectory revision: the agent realises a previous
    //    probe was wrong. Update with the corrected value at higher
    //    confidence. This is the AGM-revision moment.
    if probe_idx > 0 {
        let revise_idx = (rng.next_u64() as usize) % probe_idx;
        steps.push(TraceStep::Update {
            key: format!("probe_{revise_idx}"),
            value: format!("at {true_location}: found (revised)"),
            confidence: 0.92,
            correct: true,
        });
    }

    // 4. Final assertion: location settled.
    steps.push(TraceStep::Insert {
        key: "resolution".into(),
        value: format!("{object} located at {true_location}"),
        confidence: 0.93,
        correct: true,
    });
    let _ = container; // unused if ⌐ branch picked
    steps
}

/// WebShop-like: 10–18 steps. Asserted user intent, several Inferred
/// candidates, filter refinements that contradict candidates,
/// occasional paraphrase of the intent (exercises K\*6 semantic
/// equivalence).
fn webshop_like(trajectory_id: u32) -> Vec<TraceStep> {
    let mut rng = SplitMix::seed_from(WEBSHOP_TAG, trajectory_id);
    let categories = ["laptop", "kettle", "lamp", "headphones", "monitor"];
    let cat = rng.pick(&categories);
    let mut steps = Vec::new();

    // 1. Initial user intent.
    steps.push(TraceStep::Insert {
        key: "user_intent".into(),
        value: format!("user wants a {cat}"),
        confidence: 0.95,
        correct: true,
    });

    // 2. Candidates — 4..8 of them. Each candidate is Inferred at
    //    mid-confidence; correctness reflects whether the candidate
    //    actually matched the latent criteria.
    let n_candidates = 4 + (rng.next_u64() % 5) as usize;
    let winning_candidate = rng.next_u64() % n_candidates as u64;
    for i in 0..n_candidates {
        let is_winner = i as u64 == winning_candidate;
        let agent_right = rng.bernoulli(0.75);
        let conf = if agent_right == is_winner {
            0.75 + rng.next_f32() * 0.15
        } else {
            0.25 + rng.next_f32() * 0.20
        };
        let correct = is_winner == (conf > 0.5);
        steps.push(TraceStep::Insert {
            key: format!("candidate_{i}"),
            value: format!("product #{i} matches"),
            confidence: conf,
            correct,
        });
    }

    // 3. Paraphrase of the user intent — exercises K*6. Update the
    //    same `user_intent` key with a rephrasing; if K*6 semantic
    //    equivalence is wired, this is *not* a contradiction.
    let paraphrases = [
        "the user is looking for", "user request:", "shopper wants",
    ];
    let para = rng.pick(&paraphrases);
    steps.push(TraceStep::Update {
        key: "user_intent".into(),
        value: format!("{para} a {cat}"),
        confidence: 0.93,
        correct: true,
    });

    // 4. Filter refinements — 2..4 of them. Each filter is Asserted,
    //    and contradicts roughly half the candidates by revising them
    //    downwards.
    let n_filters = 2 + (rng.next_u64() % 3) as usize;
    for f in 0..n_filters {
        steps.push(TraceStep::Insert {
            key: format!("filter_{f}"),
            value: format!("price band #{f}"),
            confidence: 0.90,
            correct: true,
        });
        // Revise ~half the candidates downwards.
        for c in 0..n_candidates {
            if rng.bernoulli(0.5) {
                let conf = 0.10 + rng.next_f32() * 0.20;
                let correct = c as u64 != winning_candidate;
                steps.push(TraceStep::Update {
                    key: format!("candidate_{c}"),
                    value: format!("product #{c} excluded by filter_{f}"),
                    confidence: conf,
                    correct,
                });
            }
        }
    }

    // 5. Final purchase decision.
    steps.push(TraceStep::Insert {
        key: "purchase".into(),
        value: format!("purchase product #{winning_candidate}"),
        confidence: 0.92,
        correct: true,
    });
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic() {
        let a = Trace::generate(Suite::AlfworldLike, 7);
        let b = Trace::generate(Suite::AlfworldLike, 7);
        assert_eq!(a.steps.len(), b.steps.len());
        for (x, y) in a.steps.iter().zip(b.steps.iter()) {
            assert_eq!(x.key(), y.key());
            assert_eq!(x.correct(), y.correct());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let a = Trace::generate(Suite::AlfworldLike, 1);
        let b = Trace::generate(Suite::AlfworldLike, 2);
        // At least one step must differ — sanity check the RNG isn't
        // a constant.
        let same_len = a.steps.len() == b.steps.len();
        let any_diff = if same_len {
            a.steps
                .iter()
                .zip(b.steps.iter())
                .any(|(x, y)| x.key() != y.key() || x.correct() != y.correct())
        } else {
            true
        };
        assert!(any_diff);
    }

    #[test]
    fn alfworld_trace_has_goal_and_resolution() {
        let t = Trace::generate(Suite::AlfworldLike, 0);
        assert_eq!(t.steps.first().unwrap().key(), "goal");
        assert_eq!(t.steps.last().unwrap().key(), "resolution");
    }

    #[test]
    fn webshop_trace_has_intent_paraphrase_and_purchase() {
        let t = Trace::generate(Suite::WebshopLike, 0);
        let keys: Vec<&str> = t.steps.iter().map(|s| s.key()).collect();
        assert_eq!(keys.first().copied(), Some("user_intent"));
        assert_eq!(keys.last().copied(), Some("purchase"));
        // user_intent must be updated at least once (the paraphrase).
        let intent_updates = t
            .steps
            .iter()
            .filter(|s| matches!(s, TraceStep::Update { key, .. } if key == "user_intent"))
            .count();
        assert!(intent_updates >= 1, "WebShop trace must include intent paraphrase");
    }

    #[test]
    fn suite_slug_round_trip() {
        for s in [Suite::AlfworldLike, Suite::WebshopLike] {
            let parsed: Suite = s.slug().parse().unwrap();
            assert_eq!(parsed, s);
        }
    }
}
