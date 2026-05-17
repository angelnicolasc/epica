"""Tests for PyBehavioralContract — preconditions, invariants, drift bound."""

import pytest

from epica import BeliefQuad, BehavioralContract, ContractViolationError


# ── Construction ──────────────────────────────────────────────────────────────

def test_contract_construction():
    c = BehavioralContract("test_domain")
    assert c.domain == "test_domain"
    assert c.precondition_count == 0
    assert c.invariant_count == 0


def test_contract_repr():
    c = BehavioralContract("domain")
    r = repr(c)
    assert "BehavioralContract" in r
    assert "domain" in r


def test_drift_bound(contract):
    # D* = alpha/gamma = 0.05/0.5 = 0.1
    bound = contract.drift_bound()
    assert isinstance(bound, float)
    assert bound > 0.0


# ── Preconditions ─────────────────────────────────────────────────────────────

def test_empty_preconditions_always_pass(fresh_quad):
    c = BehavioralContract("empty")
    assert c.check_preconditions(fresh_quad) is True


def test_precondition_passes_when_belief_present(contract, populated_quad):
    # contract requires "environment" with confidence >= 0.5
    # populated_quad has "environment" at 0.80
    assert contract.check_preconditions(populated_quad) is True


def test_precondition_fails_when_belief_missing(contract, fresh_quad):
    assert contract.check_preconditions(fresh_quad) is False


def test_precondition_fails_when_confidence_too_low(contract):
    q = BeliefQuad()
    q.insert("environment", "production", 0.2)  # below threshold of 0.5
    assert contract.check_preconditions(q) is False


def test_precondition_raising_on_failure(contract, fresh_quad):
    with pytest.raises(ContractViolationError):
        contract.check_preconditions_raising(fresh_quad)


def test_precondition_raising_passes_silently(contract, populated_quad):
    contract.check_preconditions_raising(populated_quad)  # should not raise


def test_presence_precondition(fresh_quad):
    c = BehavioralContract("presence_test")
    c.add_presence_precondition("required_key")
    assert c.check_preconditions(fresh_quad) is False
    fresh_quad.insert("required_key", "exists", 0.0)
    assert c.check_preconditions(fresh_quad) is True


# ── Invariants ────────────────────────────────────────────────────────────────

def test_empty_invariants_always_pass(fresh_quad):
    c = BehavioralContract("empty")
    assert c.check_invariants(fresh_quad) is True


def test_invariant_passes_when_satisfied(contract, populated_quad):
    assert contract.check_invariants(populated_quad) is True


def test_invariant_fails_when_violated(contract, fresh_quad):
    # No "environment" key → invariant fails
    assert contract.check_invariants(fresh_quad) is False


def test_invariant_raising_on_failure(contract, fresh_quad):
    with pytest.raises(ContractViolationError):
        contract.check_invariants_raising(fresh_quad)


def test_invariant_severity_levels():
    q = BeliefQuad()
    c = BehavioralContract("sev_test")
    c.add_invariant("required", min_confidence=0.5, severity="soft")
    c.add_invariant("required2", min_confidence=0.5, severity="critical")

    # Soft violations don't return False from check_invariants (only Hard/Critical do)
    # Critical should be caught
    assert c.check_invariants(q) is False


def test_invariant_invalid_severity_raises():
    c = BehavioralContract("test")
    with pytest.raises(ValueError):
        c.add_invariant("key", severity="unknown_severity")


# ── Governance ────────────────────────────────────────────────────────────────

def test_set_max_tokens():
    c = BehavioralContract("gov")
    c.set_max_tokens(1000)
    # No assertion needed — just verify no exception raised


# ── Adding multiple constraints ───────────────────────────────────────────────

def test_multiple_preconditions():
    c = BehavioralContract("multi")
    c.add_precondition("key1", min_confidence=0.5)
    c.add_precondition("key2", min_confidence=0.5)
    assert c.precondition_count == 2

    q = BeliefQuad()
    q.insert("key1", "v1", 0.9)
    # key2 missing → fails
    assert c.check_preconditions(q) is False
    q.insert("key2", "v2", 0.9)
    assert c.check_preconditions(q) is True


def test_contract_check_after_revise():
    c = BehavioralContract("revision")
    c.add_invariant("belief", min_confidence=0.6, severity="hard")

    q = BeliefQuad()
    q.insert("belief", "initial", 0.9)
    assert c.check_invariants(q)

    q.revise("belief", "revised down", 0.2)  # below threshold
    assert not c.check_invariants(q)
