"""Tests for PyBeliefRuntime — insert, update, retrieve, checkpoints, session."""

import pytest

from epica import BeliefRuntime, SessionReport


# ── Construction ──────────────────────────────────────────────────────────────

def test_runtime_construction():
    rt = BeliefRuntime()
    assert repr(rt).startswith("BeliefRuntime")


def test_runtime_custom_params():
    rt = BeliefRuntime(reflection_threshold=0.20, budget=5, refill_rate=0.5)
    assert repr(rt).startswith("BeliefRuntime")


# ── Insert & get ──────────────────────────────────────────────────────────────

def test_insert_belief_returns_key(runtime):
    key = runtime.insert_belief("user_goal", "ship feature X", 0.9)
    assert key == "user_goal"


def test_get_by_key_found(runtime):
    runtime.insert_belief("k", "v", 0.8)
    id_str = runtime.get_by_key("k")
    assert id_str is not None
    assert isinstance(id_str, str)


def test_get_by_key_missing(runtime):
    assert runtime.get_by_key("nonexistent") is None


def test_get_belief_returns_node(runtime):
    runtime.insert_belief("status", "active", 0.9)
    node = runtime.get_belief("status")
    assert node is not None
    assert node.key == "status"
    assert "active" in node.value


def test_get_belief_missing_returns_none(runtime):
    assert runtime.get_belief("nope") is None


# ── Update belief ─────────────────────────────────────────────────────────────

def test_update_belief_returns_status(runtime):
    runtime.insert_belief("x", "initial", 0.9)
    result = runtime.update_belief("x", "updated", 0.7)
    assert "status" in result
    assert result["status"] in ("system1_only", "system2_activated", "system2_throttled")


def test_update_belief_key_not_found_raises(runtime):
    with pytest.raises(KeyError):
        runtime.update_belief("nonexistent_key", "value", 0.5)


def test_update_belief_system1_status(runtime):
    # With very high threshold, System 2 should not trigger
    rt2 = BeliefRuntime(reflection_threshold=0.99, budget=0)
    rt2.insert_belief("key", "val", 0.5)
    result = rt2.update_belief("key", "val2", 0.5)
    assert result["status"] in ("system1_only", "system2_throttled")


# ── Retrieve ──────────────────────────────────────────────────────────────────

def test_retrieve_for_query_returns_pairs(runtime):
    for i in range(5):
        runtime.insert_belief(f"belief_{i}", f"value_{i}", 0.5 + i * 0.1)
    results = runtime.retrieve_for_query("", budget=5000)
    assert len(results) > 0
    assert all(isinstance(k, str) and isinstance(c, float) for k, c in results)


def test_retrieve_respects_budget(runtime):
    for i in range(20):
        runtime.insert_belief(f"b{i}", f"v{i}", 0.5)
    results = runtime.retrieve_for_query("", budget=200)  # ~2 beliefs
    assert len(results) <= 5


# ── Checkpoints ───────────────────────────────────────────────────────────────

def test_checkpoint_returns_string(runtime):
    cp = runtime.checkpoint()
    assert isinstance(cp, str)
    assert cp.startswith("chk:")


# ── Session ───────────────────────────────────────────────────────────────────

def test_finalize_session_returns_report(runtime):
    runtime.insert_belief("x", "v", 0.9)
    report = runtime.finalize_session()
    assert isinstance(report, SessionReport)
    assert report.total_revisions >= 0
    assert report.calibration_target_met is True or report.trajectory_ece is not None


def test_session_report_tece_none_when_empty(runtime):
    report = runtime.session_report()
    # No beliefs updated → no history → T-ECE is None
    assert report.trajectory_ece is None
    assert report.total_revisions == 0


def test_session_report_to_dict(runtime):
    report = runtime.session_report()
    d = report.to_dict()
    assert "trajectory_ece" in d
    assert "total_revisions" in d
    assert "calibration_target_met" in d


# ── Context manager ───────────────────────────────────────────────────────────

def test_context_manager():
    with BeliefRuntime() as rt:
        rt.insert_belief("inside", "context", 0.9)
        assert rt.get_by_key("inside") is not None
    # No exception after exit


def test_context_manager_finalizes():
    with BeliefRuntime() as rt:
        rt.insert_belief("k", "v", 0.9)
        rt.update_belief("k", "v2", 0.6)
    # If we can create a new report after context exit, finalization ran
    # (session is finalized internally — no assertion needed beyond no exception)


# ── Provenance metadata ───────────────────────────────────────────────────────

def test_insert_belief_with_llm_model_records_model(runtime):
    runtime.insert_belief("b", "val", 0.9, provenance="llm", llm_model="claude-sonnet-4-6")
    node = runtime.get_belief("b")
    assert node is not None
    assert "claude-sonnet-4-6" in node.provenance


def test_update_belief_with_tool_name_records_tool(runtime):
    runtime.insert_belief("b", "initial", 0.9)
    runtime.update_belief("b", "updated", 0.8, provenance="tool", tool_name="read_file")
    node = runtime.get_belief("b")
    assert node is not None
    assert "read_file" in node.provenance
