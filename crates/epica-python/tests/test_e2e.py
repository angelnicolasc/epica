"""End-to-end integration tests — no mocks, full pipeline."""

import pytest

from epica import (
    BeliefQuad,
    BeliefRuntime,
    BehavioralContract,
    BeliefDiff,
    SessionReport,
    ContractViolationError,
)
from epica.decorators import belief_state, governed_by


# ── E2E 1: Full belief lifecycle ──────────────────────────────────────────────

def test_full_belief_lifecycle():
    """Insert → revise → checkpoint → rollback → verify diff."""
    quad = BeliefQuad()

    quad.insert("user_intent", "deploy to staging", 0.90)
    quad.insert("environment", "production", 0.80)
    quad.insert("blocker", "none", 0.95)

    cp = quad.checkpoint()
    assert cp.startswith("chk:")

    # Contradicting revision — K*4 should allow rollback
    quad.revise("user_intent", "deploy to production", 0.50)
    assert "production" in quad.get("user_intent").value

    diff = quad.rollback_to(cp)
    assert isinstance(diff, BeliefDiff)
    assert diff.has_contradictions
    assert "modified" in repr(diff) or len(diff) > 0

    # State restored
    restored = quad.get("user_intent")
    assert "staging" in restored.value


# ── E2E 2: Contract governs runtime ──────────────────────────────────────────

def test_contract_governs_quad():
    """Contract precondition fails → insert missing key → precondition passes."""
    contract = BehavioralContract("deployment_safety")
    contract.add_precondition("environment", min_confidence=0.8)
    contract.add_invariant("environment", min_confidence=0.5, severity="hard")

    quad = BeliefQuad()
    # No environment → precondition fails
    assert not contract.check_preconditions(quad)

    quad.insert("environment", "staging", 0.9)
    assert contract.check_preconditions(quad)
    assert contract.check_invariants(quad)

    # Revise environment to very low confidence → invariant fails
    quad.revise("environment", "unknown", 0.1)
    assert not contract.check_invariants(quad)


# ── E2E 3: Session report T-ECE bounded ──────────────────────────────────────

def test_session_report_tece_bounded():
    """After a multi-revision session, T-ECE should be in [0, 1] or None."""
    with BeliefRuntime(reflection_threshold=0.99, budget=0) as rt:
        # Insert and update several beliefs
        for i in range(5):
            rt.insert_belief(f"belief_{i}", f"initial_{i}", 0.5 + i * 0.1)

        for i in range(5):
            rt.update_belief(f"belief_{i}", f"updated_{i}", 0.6 + i * 0.05)

        report = rt.finalize_session()

    assert isinstance(report, SessionReport)
    if report.trajectory_ece is not None:
        assert 0.0 <= report.trajectory_ece <= 1.0
    assert report.total_revisions >= 5


# ── E2E 4: Decorator pipeline ─────────────────────────────────────────────────

def test_decorator_pipeline():
    """@belief_state + @governed_by full pipeline."""
    safety = BehavioralContract("safety")
    safety.add_precondition("approved", min_confidence=0.9)
    safety.add_invariant("approved", min_confidence=0.5, severity="hard")

    @belief_state(contract=safety)
    class DeploymentAgent:
        def __init__(self, approved: bool):
            if approved:
                self.belief_quad.insert("approved", "yes", 0.95)
            else:
                # Not inserting "approved" → precondition will fail
                pass

        @governed_by(safety)
        def deploy(self) -> str:
            return "deployed"

    # Happy path
    agent = DeploymentAgent(approved=True)
    assert agent.deploy() == "deployed"
    assert agent.check_invariants()

    # Unhappy path — __init__ fails contract check
    with pytest.raises(ContractViolationError):
        DeploymentAgent(approved=False)


# ── E2E 5: Counterfactual analysis ────────────────────────────────────────────

def test_counterfactual_analysis():
    """Insert causal chain → run counterfactual → verify survivors."""
    quad = BeliefQuad()

    quad.insert("root_cause", "auth service down", 0.9)
    quad.insert("intermediate", "api calls failing", 0.7)
    quad.insert("symptom", "users cannot login", 0.6)
    quad.insert("unrelated", "weather is sunny", 0.5)

    # Build causal chain: root → intermediate → symptom
    quad.add_causal_edge("root_cause", "intermediate")
    quad.add_causal_edge("intermediate", "symptom")

    # Counterfactual: what if root_cause had never occurred?
    result = quad.counterfactual("root_cause")

    assert result.removed_key == "root_cause"
    # "root_cause" itself is excluded
    assert "root_cause" not in result.surviving
    # "unrelated" should survive (no causal dependency)
    assert "unrelated" in result.surviving
    # "intermediate" and "symptom" are descendants → excluded
    assert "intermediate" not in result.surviving
    assert "symptom" not in result.surviving
    assert result.excluded_count >= 3  # root + 2 descendants


# ── E2E 6: Retrieve for query ─────────────────────────────────────────────────

def test_retrieve_for_query_ordering():
    """Higher-confidence beliefs should appear first in retrieval."""
    rt = BeliefRuntime()
    rt.insert_belief("low", "low value", 0.1)
    rt.insert_belief("high", "high value", 0.95)
    rt.insert_belief("mid", "medium value", 0.5)

    results = rt.retrieve_for_query("", budget=10_000)
    assert len(results) == 3
    keys_in_order = [k for k, _ in results]
    # "high" should rank above "low" (multicriteria score dominated by uncertainty)
    assert keys_in_order.index("high") < keys_in_order.index("low")
