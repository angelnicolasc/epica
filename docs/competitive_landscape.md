# Competitive Landscape

An honest comparison of Epica against alternatives in the memory and agent safety space. The goal is to show differences, not to declare winners - different tools solve different problems.

---

## Comparison table

| Feature | Vector store (e.g., Pinecone, Weaviate) | Graph memory (e.g., Zep, MemGPT) | LangGraph memory | Output guardrails (e.g., Guardrails.ai, AgentAssert) | Epica |
|---------|:------:|:------:|:------:|:------:|:------:|
| **Belief revision on contradiction** | No | No | No | No | AGM K\*2–K\*6 (hard errors) |
| **Semantic-equivalence K\*6** | No | No | No | No | Embedding-aware; paraphrases recognised on hot path |
| **Causal confidence propagation** | No | Partial (graph traversal) | No | No | Noisy-OR over CausalGraph |
| **Continuous Bayesian-surprise audit** | No | No | No | No | Friston FEP monitor (opt-in) |
| **Formal revision postulates** | No | No | No | No | K\*2–K\*6 verified |
| **Typed contracts on belief writes** | No | No | No | Output-level only | `C = (P, I, G, R)` before agent acts |
| **Memory governance (write/read/forget policy)** | No | Partial | No | No | 9 primitives (arXiv:2604.16548) |
| **Forget-policy verification** | No | No | No | No | Exhaustive graph traversal |
| **Tamper-evident audit ledger** | No | No | No | No | BLAKE3 Merkle chain + Ed25519 receipts |
| **Offline third-party verifiability** | No | No | No | No | `epica-verify` CLI |
| **Rollback with formal guard** | No | No | No | No | K\*4 vacuity enforced |
| **MCP 2026 native** | No | No | No | No | 16 routes + SEP-1686 Tasks |
| **Rust library (embeddable)** | No (service) | No (service) | No (Python framework) | No (Python) | Yes |
| **Python bindings** | Native | Native | Native | Native | PyO3 — full API including `LlmClient` injection; no native async bridge (TD-P6-001) |
| **Paper-grounded** | No | Partial | No | Partial | 5 arXiv papers (2026) + Friston FEP |
| **Implementation maturity** | Production | Production | Production | Production | Production-oriented (2026) |

---

## Detailed comparison by category

### Vector stores (Pinecone, Weaviate, Chroma, pgvector)

**Strengths**: semantic similarity retrieval at scale; mature embedding pipelines; high-throughput insert/query.

**Limitations for agent belief management**:
- No notion of contradiction: inserting a conflicting belief silently coexists with the old one.
- No causal structure: no way to express "A caused B" and propagate confidence accordingly.
- No revision semantics: update is an overwrite, not a principled revision.
- No typed constraints on what can be written.

**When to use both**: vector stores for retrieval (finding relevant memories by semantic similarity); Epica for revision (managing contradictions and confidence when beliefs conflict). They are complementary.

---

### Graph memory systems (Zep, MemGPT, Cognee)

**Strengths**: persistent memory across sessions; natural graph structure for entity relationships; some support for temporal ordering.

**Limitations**:
- No AGM revision: contradictions are handled heuristically (overwrite, merge, or ignore).
- No formal postulates: no tested guarantees about what survives a contradiction.
- No typed contracts: no enforcement of preconditions or invariants on writes.
- No confidence propagation: confidence (if present) is per-node, not propagated through graph structure.

**When to prefer Epica**: when correctness of belief revision matters more than long-term memory capacity; when you need to express causal dependencies and propagate uncertainty through them.

---

### LangGraph memory

**Strengths**: tight integration with LangGraph agent framework; flexible memory schema; good tooling for Python developers.

**Limitations**:
- Memory is session state, not belief revision: LangGraph memory stores what was said, not what the agent believes with what confidence.
- No contradiction handling: the framework does not detect when a new memory contradicts an old one.
- No formal properties: no tested revision guarantees.

**Relationship**: Epica's Python SDK includes `EpicaBeliefTool` for LangChain/LangGraph integration. Epica and LangGraph memory can coexist: LangGraph memory handles conversation history; Epica handles belief state with revision semantics.

---

### Output guardrails (Guardrails.ai, AgentAssert)

**Strengths**: inspects what the LLM said; catches policy violations in the output string; Python-native and easy to integrate.

**Limitations**:
- Output-level only: they detect violations after the agent has already acted.
- No epistemic layer: they do not inspect what the agent believes, only what it said.
- No causal structure: they cannot detect that a violation was caused by a specific upstream belief.

**Relationship**: Epica operates before the agent acts (belief mutation time). Output guardrails operate after (output string time). They address different failure modes and are complementary. In a full deployment: Epica catches belief-level drift; AgentAssert catches output-level violations.

---

### Summary

Epica's primary differentiator is **formal belief revision with verified postulates** combined with **typed contracts enforced at mutation time**. Among the tools surveyed here, no other provides both.

Where Epica is **weaker**:
- Retrieval at scale: vector stores are significantly more mature for high-volume semantic search.
- Long-term persistence: graph memory systems have more mature cross-session storage.
- Ecosystem integration: LangGraph and Guardrails have deeper framework integrations today.
- Embedding cache: the `CachedEmbeddingProvider` is unbounded in-memory; long-running agents will see memory growth without eviction (TD-P8-002).

Where Epica is **different** (not just stronger):
- It is an embeddable Rust library, not a hosted service.
- It is designed for correctness (AGM compliance) over throughput.
- It exposes the causal structure of beliefs, not just their content.

---

## Positioning

Epica is the right tool when the agent's **epistemic consistency** matters more than raw memory capacity - when you need to know not just what the agent remembers, but whether those memories are mutually consistent, how confident it should be in each given the others, and whether its beliefs have been revised correctly when new evidence arrives.

For systems where memory is primarily retrieval (find relevant past context), vector stores or graph memory are likely a better fit. For systems where memory is principled belief management (detect contradictions, enforce policies, propagate confidence), Epica addresses gaps that existing tools do not.
