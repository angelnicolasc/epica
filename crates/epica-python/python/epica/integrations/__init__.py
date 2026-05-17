"""Framework integration adapters for Epica.

Available adapters:

- ``epica.integrations.anthropic`` — ``AnthropicBeliefSession`` wraps an
  Anthropic SDK conversation with a ``BeliefRuntime`` session.
- ``epica.integrations.langchain`` — ``EpicaBeliefTool`` is a LangChain
  ``BaseTool`` that records beliefs from agent tool calls.

Each module is lazily imported to avoid hard dependencies on optional extras.
Install with: ``pip install "epica[anthropic]"`` or ``pip install "epica[langchain]"``.
"""
