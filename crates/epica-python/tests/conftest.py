"""Shared pytest fixtures for the Epica Python SDK test suite."""

import pytest

from epica import BeliefQuad, BeliefRuntime, BehavioralContract


@pytest.fixture()
def fresh_quad() -> BeliefQuad:
    """Empty BeliefQuad."""
    return BeliefQuad()


@pytest.fixture()
def populated_quad() -> BeliefQuad:
    """BeliefQuad with 5 pre-inserted beliefs."""
    q = BeliefQuad()
    q.insert("user_intent", "deploy to staging", 0.90)
    q.insert("environment", "production", 0.80)
    q.insert("blocker", "auth failing", 0.70)
    q.insert("deadline", "friday eod", 0.60)
    q.insert("team_size", "4 engineers", 0.95)
    return q


@pytest.fixture()
def runtime() -> BeliefRuntime:
    """BeliefRuntime with default thresholds."""
    return BeliefRuntime(reflection_threshold=0.15, budget=10)


@pytest.fixture()
def contract() -> BehavioralContract:
    """Minimal BehavioralContract with one precondition and one invariant."""
    c = BehavioralContract("test_contract")
    c.add_precondition("environment", min_confidence=0.5)
    c.add_invariant("environment", min_confidence=0.3, severity="hard")
    return c
