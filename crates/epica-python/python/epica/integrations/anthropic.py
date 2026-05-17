"""Anthropic SDK integration for Epica.

Wraps a ``BeliefRuntime`` session around an Anthropic SDK conversation.
Beliefs are extracted from Claude's response text via heuristic pattern matching
(no extra API calls — zero additional cost).

Requires: ``pip install "epica[anthropic]"``

Example::

    import anthropic
    from epica.integrations.anthropic import AnthropicBeliefSession

    client = anthropic.Anthropic()

    with AnthropicBeliefSession(client, model="claude-sonnet-4-6") as session:
        reply = session.message("The capital of France is Paris.")
        print(session.runtime.session_report())
"""

from __future__ import annotations

import re
from typing import Any, Optional

from epica._epica import BeliefRuntime, SessionReport


# Simple heuristic patterns for extracting belief assertions from text.
# These cover the most common forms Claude uses to state facts.
_BELIEF_PATTERNS = [
    r"(?:The\s+)?(\w[\w\s]{2,40})\s+is\s+([\w][\w\s,.-]{1,80})\.",
    r"(\w[\w\s]{2,40})\s+=\s+([\w][\w\s,.-]{1,80})\.",
    r"I (?:know|believe|understand) that\s+(.{5,80})\s+is\s+([\w][\w\s,.-]{1,80})\.",
]
_CONFIDENCE_DEFAULT = 0.75


def _extract_beliefs(text: str) -> list[tuple[str, str, float]]:
    """Heuristically extract (key, value, confidence) triples from response text."""
    results: list[tuple[str, str, float]] = []
    for pattern in _BELIEF_PATTERNS:
        for match in re.finditer(pattern, text, re.IGNORECASE):
            key = match.group(1).strip().lower().replace(" ", "_")
            value = match.group(2).strip().rstrip(".")
            if 3 <= len(key) <= 50 and len(value) <= 100:
                results.append((key, value, _CONFIDENCE_DEFAULT))
    return results


class AnthropicBeliefSession:
    """Wraps a ``BeliefRuntime`` around an Anthropic SDK conversation.

    Every message exchange is captured: user messages insert beliefs with
    provenance ``"user"``, and extracted facts from Claude's replies are
    inserted with provenance ``"llm"``.

    The session is finalised (T-ECE computed) when the context manager exits.

    Args:
        client: An instantiated ``anthropic.Anthropic()`` client.
        model: Claude model ID (default: ``"claude-sonnet-4-6"``).
        reflection_threshold: System 2 activation threshold (default: 0.15).
        budget: System 2 token budget (default: 50).
    """

    def __init__(
        self,
        client: Any,
        model: str = "claude-sonnet-4-6",
        reflection_threshold: float = 0.15,
        budget: int = 50,
        system: Optional[str] = None,
    ) -> None:
        try:
            import anthropic as _anthropic  # noqa: F401
        except ImportError:
            raise ImportError(
                "The 'anthropic' package is required. Install with: pip install 'epica[anthropic]'"
            )
        self._client = client
        self._model = model
        self._system = system
        self.runtime = BeliefRuntime(reflection_threshold=reflection_threshold, budget=budget)
        self._history: list[dict[str, str]] = []

    def message(self, user_text: str, max_tokens: int = 1024) -> str:
        """Send a message and return Claude's reply.

        Extracted facts from the reply are automatically inserted into the
        ``BeliefRuntime`` with provenance ``"llm"``.
        """
        self._history.append({"role": "user", "content": user_text})
        kwargs: dict[str, Any] = {
            "model": self._model,
            "max_tokens": max_tokens,
            "messages": self._history,
        }
        if self._system:
            kwargs["system"] = self._system
        response = self._client.messages.create(**kwargs)
        reply = response.content[0].text
        self._history.append({"role": "assistant", "content": reply})

        # Extract and record beliefs from the reply.
        for key, value, confidence in _extract_beliefs(reply):
            self.runtime.insert_belief(key, value, confidence, provenance="llm")

        return reply

    def finalize(self) -> SessionReport:
        """Finalize the session and return the report."""
        return self.runtime.finalize_session()

    def __enter__(self) -> AnthropicBeliefSession:
        return self

    def __exit__(self, *_: Any) -> bool:
        self.runtime.finalize_session()
        return False
