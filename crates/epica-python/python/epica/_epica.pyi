"""Type stubs for epica._epica (the Rust extension module).

These stubs provide full IDE completion and mypy/pyright type checking.
They are hand-crafted to be more precise than auto-generated stubs.
"""

from __future__ import annotations

from typing import Iterator, Optional

__version__: str
__author__: str

__all__: list[str]

# ── Exceptions ────────────────────────────────────────────────────────────────

class EpicaError(Exception):
    """Base exception for all Epica errors."""

class BeliefRevisionError(EpicaError):
    """Raised when AGM belief revision fails (postulate violation, key not found)."""

class ContractViolationError(EpicaError):
    """Raised when a behavioral contract precondition or invariant is violated."""

class CheckpointError(EpicaError):
    """Raised when a checkpoint is not found or K*4 guard rejects the rollback."""

class SovereigntyError(EpicaError):
    """Raised when a mnemonic sovereignty governance policy blocks the operation."""

# ── Data classes ──────────────────────────────────────────────────────────────

class BeliefNode:
    """A snapshot of a single belief node. Read-only."""

    key: str
    """Domain-scoped key, e.g. ``'user_intent'`` or ``'file_tree'``."""

    value: str
    """Human-readable representation of the belief value."""

    value_kind: str
    """Value kind: ``'asserted'``, ``'inferred'``, ``'deterministic'``, or ``'reference'``."""

    provenance: str
    """Provenance: ``'user_statement'``, ``'llm_inference:<model>'``,
    ``'tool_result:<tool>'``, or ``'runtime_inference'``."""

    fast_confidence: float
    """System 1 (fast) confidence — always in ``[0.0, 1.0]``."""

    slow_confidence: Optional[float]
    """System 2 (slow) confidence — ``None`` until System 2 has reflected."""

    created_at_ms: int
    """Creation timestamp as milliseconds since UNIX epoch."""

    effective_confidence: float
    """``slow_confidence`` when set, otherwise ``fast_confidence``."""

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...


class BeliefDiff:
    """Structural diff between two ``BeliefQuad`` states. Read-only."""

    added: list[str]
    """Keys of beliefs added since the checkpoint."""

    removed: list[str]
    """Keys of beliefs removed since the checkpoint."""

    modified: list[str]
    """Keys of beliefs whose value or confidence changed since the checkpoint."""

    has_contradictions: bool
    """``True`` when any of ``added``, ``modified``, or ``removed`` are non-empty."""

    trajectory_ece: Optional[float]
    """Trajectory-ECE for this diff interval, if available."""

    def __len__(self) -> int:
        """Total number of changed beliefs (added + removed + modified)."""
        ...

    def is_empty(self) -> bool: ...
    def __repr__(self) -> str: ...
    def to_dict(self) -> dict[str, object]: ...


class CounterfactualResult:
    """Surviving beliefs after a counterfactual removal. Read-only."""

    removed_key: str
    """The key of the removed antecedent belief."""

    surviving: list[str]
    """Keys of all beliefs that survive in this counterfactual world."""

    excluded_count: int
    """Number of beliefs excluded (antecedent + its causal descendants)."""

    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...


class SessionReport:
    """Summary of a completed ``BeliefRuntime`` session. Read-only."""

    trajectory_ece: Optional[float]
    """T-ECE: ``Σ_t |confidence_t − accuracy_t| / T``. ``None`` until finalized."""

    total_revisions: int
    """Total number of ``update_belief()`` calls in this session."""

    contradictions_detected: int
    """Number of revisions where AGM contraction fired."""

    system2_activations: int
    """Number of times System 2 (LLM reflection) was activated."""

    system2_throttled: int
    """Number of times System 2 was skipped due to budget exhaustion."""

    calibration_target_met: bool
    """``True`` when ``trajectory_ece < 0.08``."""

    soft_violations: int
    """Number of soft invariant violations logged."""

    hard_violations: int
    """Number of hard invariant violations that triggered recovery."""

    critical_violations: int
    """Number of critical invariant violations that halted execution."""

    recovery_actions_taken: int
    """Number of recovery actions executed."""

    governance_tokens_used: int
    """Governance tokens consumed this session."""

    def __repr__(self) -> str: ...
    def to_dict(self) -> dict[str, object]: ...


# ── Core classes ──────────────────────────────────────────────────────────────

class BeliefQuad:
    """Four-graph belief store with formal AGM belief revision.

    Thread-safe — internally wrapped in ``RwLock`` for GIL-free Python 3.13+.

    Example::

        quad = BeliefQuad()
        quad.insert("user_intent", "deploy to staging", 0.95)
        quad.insert("environment", "production", 0.80)
        quad.add_causal_edge("environment", "user_intent")

        cp = quad.checkpoint()
        quad.revise("user_intent", "deploy to production", 0.70)

        diff = quad.rollback_to(cp)
        print(diff)  # BeliefDiff(+0 -0 ~1, contradictions=True)
    """

    def __init__(self) -> None: ...

    # CRUD
    def insert(
        self,
        key: str,
        value: str,
        confidence: float,
        provenance: Optional[str] = None,
        llm_model: Optional[str] = None,
        tool_name: Optional[str] = None,
    ) -> None:
        """Insert a belief.

        ``provenance`` is one of ``'user'``, ``'llm'``, ``'tool'``.
        ``llm_model`` names the model when ``provenance='llm'`` (e.g. ``'claude-opus-4-7'``).
        ``tool_name`` names the tool when ``provenance='tool'`` (e.g. ``'bash'``).
        """
        ...

    def get(self, key: str) -> Optional[BeliefNode]:
        """Return the belief node for ``key``, or ``None`` if not found."""
        ...

    def remove(self, key: str) -> bool:
        """Remove a belief. Returns ``True`` if the key existed."""
        ...

    # AGM revision
    def revise(
        self,
        key: str,
        new_value: str,
        confidence: float,
        provenance: Optional[str] = None,
        llm_model: Optional[str] = None,
        tool_name: Optional[str] = None,
    ) -> None:
        """Revise the belief at ``key`` using the Levi identity.

        Raises ``BeliefRevisionError`` if the key does not exist.
        ``llm_model`` / ``tool_name`` are forwarded to the provenance record.
        """
        ...

    # Checkpoints
    def checkpoint(self) -> str:
        """Save the current state. Returns a checkpoint ID string."""
        ...

    def rollback_to(self, checkpoint_id: str) -> BeliefDiff:
        """Roll back to a checkpoint. Raises ``CheckpointError`` on failure."""
        ...

    def list_checkpoints(self) -> list[str]:
        """Return all stored checkpoint ID strings."""
        ...

    def diff_with_checkpoint(self, checkpoint_id: str) -> BeliefDiff:
        """Compute the diff between current state and a checkpoint."""
        ...

    # Counterfactual
    def counterfactual(self, key: str) -> CounterfactualResult:
        """Return surviving beliefs if ``key`` had never been inserted.

        Raises ``BeliefRevisionError`` if the key does not exist.
        """
        ...

    # Graph edges
    def add_semantic_edge(
        self,
        from_key: str,
        to_key: str,
        edge_type: str,  # "subsumes" | "contradicts" | "synonymous"
    ) -> None: ...

    def add_causal_edge(
        self,
        cause_key: str,
        effect_key: str,
        effect_size: float = 1.0,
        confidence: float = 1.0,
    ) -> None: ...

    def add_temporal_edge(self, earlier_key: str, later_key: str) -> None: ...

    # Bulk
    def keys(self) -> list[str]: ...
    def values(self) -> list[BeliefNode]: ...
    def items(self) -> list[tuple[str, BeliefNode]]: ...

    # Dict protocol
    def __getitem__(self, key: str) -> BeliefNode:
        """Return the belief node for ``key``. Raises ``KeyError`` if not found."""
        ...

    def __setitem__(self, key: str, value: str) -> None:
        """Insert ``key`` with ``confidence=1.0`` and ``provenance='user'``.

        For fine-grained control use ``insert()`` or ``revise()`` directly.
        """
        ...

    def __delitem__(self, key: str) -> None:
        """Remove a belief by key. Raises ``KeyError`` if not found."""
        ...

    def __iter__(self) -> Iterator[str]:
        """Iterate over all belief keys."""
        ...

    # Dunder
    def __len__(self) -> int: ...
    def __contains__(self, key: str) -> bool: ...
    def __repr__(self) -> str: ...
    def __bool__(self) -> bool: ...
    def is_empty(self) -> bool: ...
    def len(self) -> int: ...


class BeliefRuntime:
    """Async ``BeliefRuntime`` wrapped for synchronous Python access.

    Orchestrates System 1 (fast, Noisy-OR propagation) and System 2 (slow,
    LLM-based reflection). Supports behavioral contracts, session reporting,
    and Trajectory-ECE calibration measurement.

    Implements the context manager protocol — ``__exit__`` finalizes the session.

    Example::

        with BeliefRuntime(reflection_threshold=0.15, budget=50) as rt:
            rt.insert_belief("user_goal", "ship feature X", 0.9)
            rt.update_belief("user_goal", "ship feature Y", 0.6)
            report = rt.finalize_session()
    """

    def __init__(
        self,
        reflection_threshold: float = 0.15,
        budget: int = 50,
        refill_rate: float = 1.0,
    ) -> None: ...

    # Belief operations
    def insert_belief(
        self,
        key: str,
        value: str,
        confidence: float,
        provenance: Optional[str] = None,
        llm_model: Optional[str] = None,
        tool_name: Optional[str] = None,
    ) -> str:
        """Insert a belief. Returns ``key`` (the stable Python-facing ID).

        ``llm_model`` names the LLM when ``provenance='llm'`` (e.g. ``'claude-opus-4-7'``).
        ``tool_name`` names the tool when ``provenance='tool'`` (e.g. ``'bash'``).
        """
        ...

    def get_by_key(self, key: str) -> Optional[str]:
        """Return the opaque internal ID for ``key``, or ``None``."""
        ...

    def get_belief(self, key: str) -> Optional[BeliefNode]:
        """Return a ``BeliefNode`` snapshot for ``key``, or ``None``."""
        ...

    def update_belief(
        self,
        key: str,
        new_value: str,
        confidence: float,
        provenance: Optional[str] = None,
        llm_model: Optional[str] = None,
        tool_name: Optional[str] = None,
    ) -> dict[str, object]:
        """Update a belief and run System 1 (+optionally System 2).

        Returns a dict with keys:
        - ``"status"``: ``"system1_only"`` | ``"system2_activated"`` | ``"system2_throttled"``
        - ``"task_id"``: task UUID string or ``None``

        ``llm_model`` / ``tool_name`` are forwarded to the provenance record.

        Raises ``KeyError`` if the key does not exist.
        Raises ``ContractViolationError`` on contract failure.
        """
        ...

    def retrieve_for_query(
        self,
        query: str,
        budget: int = 1000,
    ) -> list[tuple[str, float]]:
        """Return ``(key, confidence)`` pairs sorted by multicriteria score."""
        ...

    # Checkpoints
    def checkpoint(self) -> str: ...
    def rollback_to(self, checkpoint_id: str) -> BeliefDiff: ...

    # Session
    def finalize_session(self) -> SessionReport:
        """Mark surviving beliefs as correct and return the session report."""
        ...

    def session_report(self) -> SessionReport:
        """Return the current session report without finalizing."""
        ...

    # Context manager
    def __enter__(self) -> BeliefRuntime: ...
    def __exit__(
        self,
        exc_type: Optional[type],
        exc_val: Optional[BaseException],
        exc_tb: object,
    ) -> bool: ...

    def __repr__(self) -> str: ...


class BehavioralContract:
    """Typed behavioral contract: ``C = (P, I, G, R)``.

    From Agent Behavioral Contracts (arXiv:2602.22302).

    Enforces (p, δ, k)-satisfaction:
    probability ``p`` that no more than ``δ`` violations occur in ``k`` steps.

    Example::

        contract = BehavioralContract("deployment_safety")
        contract.add_precondition("environment", min_confidence=0.8)
        contract.add_invariant("user_goal", min_confidence=0.5, severity="hard")
        contract.check_preconditions_raising(quad)
    """

    def __init__(
        self,
        domain: str,
        alpha: float = 0.05,
        gamma: float = 0.5,
    ) -> None: ...

    # Preconditions
    def add_precondition(self, key: str, min_confidence: float = 0.5) -> None: ...
    def add_presence_precondition(self, key: str) -> None: ...

    # Invariants
    def add_invariant(
        self,
        key: str,
        min_confidence: float = 0.5,
        severity: str = "hard",  # "soft" | "hard" | "critical"
    ) -> None: ...

    # Governance
    def set_max_tokens(self, limit: int) -> None: ...

    # Evaluation
    def check_preconditions(self, quad: BeliefQuad) -> bool: ...
    def check_preconditions_raising(self, quad: BeliefQuad) -> None: ...
    def check_invariants(self, quad: BeliefQuad) -> bool: ...
    def check_invariants_raising(self, quad: BeliefQuad) -> None: ...
    def drift_bound(self) -> float: ...

    # Properties
    @property
    def domain(self) -> str: ...
    @property
    def precondition_count(self) -> int: ...
    @property
    def invariant_count(self) -> int: ...

    def __repr__(self) -> str: ...
