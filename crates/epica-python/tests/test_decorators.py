"""Tests for @belief_state and @governed_by decorators."""

import pytest

from epica import BeliefQuad, BehavioralContract, ContractViolationError
from epica.decorators import belief_state, governed_by


# ── @belief_state ─────────────────────────────────────────────────────────────

def test_belief_state_adds_quad():
    @belief_state()
    class Agent:
        def __init__(self):
            pass

    agent = Agent()
    assert hasattr(agent, "belief_quad")
    assert isinstance(agent.belief_quad, BeliefQuad)


def test_belief_state_quad_starts_empty():
    @belief_state()
    class Agent:
        def __init__(self):
            pass

    agent = Agent()
    assert agent.belief_quad.is_empty()


def test_belief_state_init_populates_quad():
    @belief_state()
    class Agent:
        def __init__(self, env: str):
            self.belief_quad.insert("environment", env, 1.0)

    agent = Agent("production")
    assert "environment" in agent.belief_quad


def test_belief_state_check_invariants_method():
    @belief_state()
    class Agent:
        def __init__(self):
            pass

    agent = Agent()
    assert agent.check_invariants() is True  # no contract → always True


def test_belief_state_with_contract_passes():
    contract = BehavioralContract("test")
    contract.add_precondition("env", min_confidence=0.5)

    @belief_state(contract=contract)
    class Agent:
        def __init__(self):
            self.belief_quad.insert("env", "staging", 0.9)

    agent = Agent()  # should not raise


def test_belief_state_with_contract_fails():
    contract = BehavioralContract("test")
    contract.add_precondition("required", min_confidence=0.5)

    @belief_state(contract=contract)
    class Agent:
        def __init__(self):
            pass  # does NOT insert "required"

    with pytest.raises(ContractViolationError):
        Agent()


# ── @governed_by ──────────────────────────────────────────────────────────────

def test_governed_by_passes_when_preconditions_met():
    contract = BehavioralContract("gov")
    contract.add_precondition("ready", min_confidence=0.5)

    @belief_state()
    class Worker:
        def __init__(self):
            self.belief_quad.insert("ready", "yes", 0.9)

        @governed_by(contract)
        def do_work(self) -> str:
            return "done"

    w = Worker()
    assert w.do_work() == "done"


def test_governed_by_raises_when_preconditions_fail():
    contract = BehavioralContract("gov")
    contract.add_precondition("required", min_confidence=0.5)

    @belief_state()
    class Worker:
        def __init__(self):
            pass  # "required" not present

        @governed_by(contract)
        def do_work(self) -> str:
            return "done"

    w = Worker()
    with pytest.raises(ContractViolationError):
        w.do_work()


def test_governed_by_preserves_return_value():
    contract = BehavioralContract("noop")

    @belief_state()
    class Worker:
        @governed_by(contract)
        def compute(self) -> int:
            return 42

    w = Worker()
    assert w.compute() == 42


def test_governed_by_without_belief_state_raises():
    contract = BehavioralContract("noop")

    class NakedWorker:
        @governed_by(contract)
        def action(self):
            pass

    with pytest.raises(AttributeError):
        NakedWorker().action()


def test_governed_by_invariant_checked_after_call():
    contract = BehavioralContract("post_check")
    contract.add_invariant("status", min_confidence=0.5, severity="hard")

    @belief_state(contract=contract)
    class Worker:
        def __init__(self):
            self.belief_quad.insert("status", "ok", 0.9)

        @governed_by(contract)
        def degrade(self):
            # Revise status to very low confidence — violates invariant
            self.belief_quad.revise("status", "degraded", 0.1)

    w = Worker()
    with pytest.raises(ContractViolationError):
        w.degrade()
