"""LangChain / LangGraph integration for Epica.

Provides ``EpicaBeliefTool`` — a standard ``BaseTool`` that records belief
assertions from LangChain agent tool calls into a ``BeliefRuntime``.

Requires: ``pip install "epica[langchain]"``

Example::

    from epica import BeliefRuntime
    from epica.integrations.langchain import EpicaBeliefTool

    runtime = BeliefRuntime()
    tool = EpicaBeliefTool(runtime=runtime)

    # Use in a LangChain agent:
    agent = create_tool_calling_agent(llm, tools=[tool], prompt=prompt)
"""

from __future__ import annotations

from typing import Any, Optional, Type

from epica._epica import BeliefRuntime


class EpicaBeliefTool:
    """LangChain ``BaseTool`` that records beliefs into a ``BeliefRuntime``.

    Input schema (JSON object):
    - ``key`` (str): belief domain key
    - ``value`` (str): belief value string
    - ``confidence`` (float, optional): confidence in ``[0, 1]`` (default: 0.8)
    - ``provenance`` (str, optional): ``"user"`` | ``"llm"`` | ``"tool"`` (default: ``"tool"``)

    Args:
        runtime: The ``BeliefRuntime`` to record beliefs into.
        name: Tool name exposed to the LLM (default: ``"record_belief"``).
    """

    name: str = "record_belief"
    description: str = (
        "Record a belief assertion into the Epica belief runtime. "
        "Use this to persist facts the agent has established. "
        "Input: {key, value, confidence?, provenance?}."
    )

    def __init__(self, runtime: BeliefRuntime, name: str = "record_belief") -> None:
        try:
            from langchain_core.tools import BaseTool  # noqa: F401
        except ImportError:
            raise ImportError(
                "The 'langchain-core' package is required. Install with: pip install 'epica[langchain]'"
            )
        self.runtime = runtime
        self.name = name

    def _run(
        self,
        key: str,
        value: str,
        confidence: float = 0.8,
        provenance: str = "tool",
    ) -> str:
        """Record a belief and return a confirmation string."""
        inserted_key = self.runtime.insert_belief(key, value, confidence, provenance=provenance)
        return f"Belief recorded: {inserted_key!r} = {value!r} (confidence={confidence:.2f})"

    def __call__(self, input: Any) -> str:
        if isinstance(input, dict):
            return self._run(
                key=input["key"],
                value=input["value"],
                confidence=float(input.get("confidence", 0.8)),
                provenance=str(input.get("provenance", "tool")),
            )
        return self._run(key=str(input), value=str(input))

    def as_langchain_tool(self) -> Any:
        """Return a proper ``langchain_core.tools.Tool`` instance."""
        from langchain_core.tools import Tool  # type: ignore[import]
        return Tool(
            name=self.name,
            description=self.description,
            func=self._run,
        )
