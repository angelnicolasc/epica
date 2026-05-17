"""Epica — Epistemic Causal Agent Belief Runtime.

Formal AGM belief revision for LLM agents, powered by Rust.

Quick start::

    from epica import BeliefQuad, BeliefRuntime, BehavioralContract

    # Low-level quad: four-graph belief store with AGM revision
    quad = BeliefQuad()
    quad.insert("user_intent", "deploy to staging", 0.95)
    quad.insert("environment", "production", 0.80)
    cp = quad.checkpoint()
    quad.revise("user_intent", "deploy to production", 0.70)
    diff = quad.rollback_to(cp)

    # High-level runtime: dual-process System 1 + 2, session reporting
    with BeliefRuntime(reflection_threshold=0.15, budget=50) as rt:
        rt.insert_belief("user_goal", "ship feature X", 0.9)
        rt.update_belief("user_goal", "ship feature Y", 0.6)
        report = rt.finalize_session()
        print(f"T-ECE: {report.trajectory_ece}")

    # Behavioral contracts: C = (P, I, G, R)
    contract = BehavioralContract("safety")
    contract.add_precondition("environment", min_confidence=0.8)
    contract.check_preconditions_raising(quad)

See `epica.decorators` for ``@belief_state`` and ``@governed_by`` decorators.
See `epica.integrations` for Anthropic SDK and LangChain adapters.
"""

from epica._epica import (  # noqa: F401
    # Core classes
    BeliefQuad,
    BeliefRuntime,
    BehavioralContract,
    # Data classes
    BeliefNode,
    BeliefDiff,
    CounterfactualResult,
    SessionReport,
    # Exceptions
    EpicaError,
    BeliefRevisionError,
    ContractViolationError,
    CheckpointError,
    SovereigntyError,
)
from epica.decorators import belief_state, governed_by  # noqa: F401

__version__ = "0.6.0"
__all__ = [
    # Core
    "BeliefQuad",
    "BeliefRuntime",
    "BehavioralContract",
    # Data
    "BeliefNode",
    "BeliefDiff",
    "CounterfactualResult",
    "SessionReport",
    # Decorators
    "belief_state",
    "governed_by",
    # Exceptions
    "EpicaError",
    "BeliefRevisionError",
    "ContractViolationError",
    "CheckpointError",
    "SovereigntyError",
]
