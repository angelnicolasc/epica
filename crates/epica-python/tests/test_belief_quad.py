"""Tests for PyBeliefQuad — CRUD, AGM revision, checkpoints, graphs."""

import pytest

from epica import BeliefQuad, BeliefDiff, BeliefRevisionError, CheckpointError


# ── CRUD ──────────────────────────────────────────────────────────────────────

def test_insert_and_len(fresh_quad):
    assert len(fresh_quad) == 0
    fresh_quad.insert("key1", "value1", 0.9)
    assert len(fresh_quad) == 1
    fresh_quad.insert("key2", "value2", 0.8)
    assert len(fresh_quad) == 2


def test_is_empty_and_bool(fresh_quad):
    assert fresh_quad.is_empty()
    assert not fresh_quad
    fresh_quad.insert("x", "y", 0.5)
    assert not fresh_quad.is_empty()
    assert fresh_quad


def test_get_existing_key(populated_quad):
    node = populated_quad.get("user_intent")
    assert node is not None
    assert node.key == "user_intent"
    assert "deploy" in node.value
    assert 0.0 <= node.fast_confidence <= 1.0


def test_get_missing_key_returns_none(fresh_quad):
    assert fresh_quad.get("nonexistent") is None


def test_contains_dunder(populated_quad):
    assert "user_intent" in populated_quad
    assert "nonexistent" not in populated_quad


def test_remove_existing(populated_quad):
    assert populated_quad.remove("blocker") is True
    assert "blocker" not in populated_quad
    assert len(populated_quad) == 4


def test_remove_missing_returns_false(fresh_quad):
    assert fresh_quad.remove("nope") is False


def test_keys_returns_all_keys(populated_quad):
    keys = populated_quad.keys()
    assert set(keys) == {"user_intent", "environment", "blocker", "deadline", "team_size"}


def test_values_returns_belief_nodes(populated_quad):
    nodes = populated_quad.values()
    assert len(nodes) == 5
    assert all(hasattr(n, "key") and hasattr(n, "fast_confidence") for n in nodes)


def test_items_returns_pairs(populated_quad):
    items = populated_quad.items()
    assert len(items) == 5
    assert all(isinstance(k, str) for k, _ in items)


def test_repr(populated_quad):
    r = repr(populated_quad)
    assert "BeliefQuad" in r
    assert "5" in r  # belief count


# ── AGM revision ──────────────────────────────────────────────────────────────

def test_revise_updates_value(fresh_quad):
    fresh_quad.insert("status", "pending", 0.9)
    fresh_quad.revise("status", "complete", 0.95)
    node = fresh_quad.get("status")
    assert node is not None
    assert "complete" in node.value


def test_revise_missing_key_raises(fresh_quad):
    with pytest.raises(BeliefRevisionError):
        fresh_quad.revise("nonexistent", "value", 0.9)


def test_revise_clamps_confidence(fresh_quad):
    fresh_quad.insert("c", "v", 0.5)
    fresh_quad.revise("c", "w", 2.0)  # should clamp to 1.0
    node = fresh_quad.get("c")
    assert node is not None
    assert node.fast_confidence <= 1.0


# ── Checkpoints ───────────────────────────────────────────────────────────────

def test_checkpoint_returns_string(fresh_quad):
    fresh_quad.insert("x", "y", 0.5)
    cp = fresh_quad.checkpoint()
    assert isinstance(cp, str)
    assert len(cp) > 0


def test_checkpoint_and_rollback(fresh_quad):
    fresh_quad.insert("initial", "state", 0.9)
    cp = fresh_quad.checkpoint()

    # Make a contradicting change
    fresh_quad.revise("initial", "changed state", 0.3)
    node_after = fresh_quad.get("initial")
    assert node_after is not None
    assert "changed" in node_after.value

    diff = fresh_quad.rollback_to(cp)
    assert isinstance(diff, BeliefDiff)
    assert diff.has_contradictions
    node_restored = fresh_quad.get("initial")
    assert node_restored is not None
    assert "state" in node_restored.value


def test_rollback_invalid_id_raises(fresh_quad):
    with pytest.raises(CheckpointError):
        fresh_quad.rollback_to("chk:00000000-0000-0000-0000-000000000000")


def test_list_checkpoints(fresh_quad):
    fresh_quad.insert("x", "y", 0.5)
    assert len(fresh_quad.list_checkpoints()) == 0
    fresh_quad.checkpoint()
    fresh_quad.checkpoint()
    assert len(fresh_quad.list_checkpoints()) == 2


def test_diff_with_checkpoint(fresh_quad):
    fresh_quad.insert("a", "1", 0.9)
    cp = fresh_quad.checkpoint()
    fresh_quad.insert("b", "2", 0.8)
    diff = fresh_quad.diff_with_checkpoint(cp)
    assert "b" in diff.added


# ── Counterfactual ────────────────────────────────────────────────────────────

def test_counterfactual_returns_result(populated_quad):
    result = populated_quad.counterfactual("user_intent")
    assert result.removed_key == "user_intent"
    assert isinstance(result.surviving, list)
    # At least the other 4 beliefs survive (no causal edges → no descendants)
    assert len(result.surviving) >= 4


def test_counterfactual_missing_key_raises(fresh_quad):
    with pytest.raises(BeliefRevisionError):
        fresh_quad.counterfactual("nonexistent")


def test_counterfactual_with_causal_chain(fresh_quad):
    fresh_quad.insert("cause", "root event", 0.9)
    fresh_quad.insert("effect", "downstream event", 0.7)
    fresh_quad.add_causal_edge("cause", "effect")

    result = fresh_quad.counterfactual("cause")
    # "effect" is a descendant — should NOT survive
    assert "effect" not in result.surviving
    assert result.excluded_count >= 2  # cause + effect


# ── Graph edges ───────────────────────────────────────────────────────────────

def test_add_semantic_edge(populated_quad):
    populated_quad.add_semantic_edge("user_intent", "deadline", "subsumes")
    # Edge add should not raise; side effects visible via counterfactual


def test_add_semantic_edge_invalid_type_raises(populated_quad):
    with pytest.raises(ValueError):
        populated_quad.add_semantic_edge("user_intent", "deadline", "invalid_type")


def test_add_causal_edge(fresh_quad):
    fresh_quad.insert("a", "v1", 0.9)
    fresh_quad.insert("b", "v2", 0.8)
    fresh_quad.add_causal_edge("a", "b", effect_size=0.7, confidence=0.9)


def test_add_temporal_edge(fresh_quad):
    fresh_quad.insert("earlier", "first event", 0.9)
    fresh_quad.insert("later", "second event", 0.8)
    fresh_quad.add_temporal_edge("earlier", "later")


def test_add_edge_missing_key_raises(fresh_quad):
    fresh_quad.insert("a", "v", 0.9)
    with pytest.raises(KeyError):
        fresh_quad.add_causal_edge("a", "nonexistent")


# ── Dict protocol ─────────────────────────────────────────────────────────────

def test_getitem_existing(populated_quad):
    node = populated_quad["user_intent"]
    assert node.key == "user_intent"
    assert "deploy" in node.value


def test_getitem_missing_raises(fresh_quad):
    with pytest.raises(KeyError):
        _ = fresh_quad["nonexistent"]


def test_setitem_inserts(fresh_quad):
    fresh_quad["intent"] = "deploy"
    node = fresh_quad["intent"]
    assert node.value == "deploy"
    assert node.fast_confidence == 1.0


def test_delitem_existing(populated_quad):
    del populated_quad["blocker"]
    assert "blocker" not in populated_quad


def test_delitem_missing_raises(fresh_quad):
    with pytest.raises(KeyError):
        del fresh_quad["nonexistent"]


def test_iter_yields_all_keys(populated_quad):
    keys = list(populated_quad)
    assert set(keys) == {"user_intent", "environment", "blocker", "deadline", "team_size"}


# ── Provenance metadata ───────────────────────────────────────────────────────

def test_insert_with_llm_model_records_model(fresh_quad):
    fresh_quad.insert("belief", "val", 0.9, provenance="llm", llm_model="claude-opus-4-7")
    node = fresh_quad.get("belief")
    assert node is not None
    assert "claude-opus-4-7" in node.provenance


def test_insert_with_tool_name_records_tool(fresh_quad):
    fresh_quad.insert("result", "output", 0.8, provenance="tool", tool_name="bash")
    node = fresh_quad.get("result")
    assert node is not None
    assert "bash" in node.provenance
