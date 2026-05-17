# MCP Server

Reference: MCP Roadmap 2026-03-05, SEP-1686.

## What this document covers

The actual implementation status of the `epica-mcp` server: which endpoints exist, which are wired to the runtime, which are tested end-to-end, and where the current gaps are.

## What is implemented today

The server is a full Axum-based MCP 2026 implementation with 16 routes. All handlers exist and are wired to the runtime. 29 E2E tests pass via the Axum test client.

## What is still approximate or planned

- SEP-1686 Tasks appear immediately as `Completed` because System 2 is synchronous. The call-now / fetch-later pattern is structurally correct but the "pending" state is never observed in current tests (TD-P5-002).
- The in-memory `TaskStore` loses tasks on server restart (TD-P5-002).
- OAuth 2.1 uses HS256 by default; RS256 is available via `EPICA_JWT_ALG=rs256` with JWKS rotation.

---

## Start

```bash
# Development (no auth)
EPICA_NO_AUTH=1 cargo run --bin epica-serve

# Production
EPICA_JWT_SECRET=your-secret \
EPICA_CONTRACTS_FILE=contracts/codebase.toml \
cargo run --bin epica-serve -- --port 8080
```

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `EPICA_ADDR` | `0.0.0.0:8765` | Bind address |
| `EPICA_NO_AUTH` | unset | Skip JWT validation if set |
| `EPICA_JWT_SECRET` | - | HS256 signing secret |
| `EPICA_JWT_ALG` | `hs256` | `hs256` or `rs256` |
| `EPICA_JWT_RSA_PEM` | - | RS256 public key PEM |
| `EPICA_RATE_LIMIT_RPS` | `100` | Requests per second per IP |
| `EPICA_SYSTEM2_BUDGET` | `50` | Max LLM reflections per session |
| `EPICA_CORS_ORIGINS` | `*` in dev | Allowed origins |
| `EPICA_CONTRACTS_FILE` | unset | TOML contract config path |

---

## Endpoints

| Endpoint | Handler exists | Wired to runtime | Tested E2E | Notes |
|----------|:--------------:|:----------------:|:----------:|-------|
| `POST /v1/beliefs` | Yes | Yes | Yes | Insert + System 1; returns `task_id` if System 2 fires |
| `GET /v1/beliefs/:key` | Yes | Yes | Yes | Value + dual-process confidence + provenance |
| `PATCH /v1/beliefs/:key` | Yes | Yes | Yes | AGM revision + System 1 + optional System 2 |
| `GET /v1/diff` | Yes | Yes | Yes | Full `BeliefQuadDiff` with T-ECE |
| `POST /v1/checkpoints` | Yes | Yes | Yes | Save immutable snapshot |
| `POST /v1/rollback` | Yes | Yes | Yes | Restore + AGM K\*4 guard + diff |
| `GET /v1/counterfactual/:key` | Yes | Yes | Yes | CausalGraph traversal - no LLM |
| `POST /v1/query` | Yes | Yes | Yes | Multicriteria retrieval (prospective * 0.45 + uncertainty * 0.25 + centrality * 0.20 - decay * 0.10) |
| `GET /v1/contract-status` | Yes | Yes | Yes | Drift bounds `D* = alpha/gamma` in real time |
| `GET /v1/tasks/:id` | Yes | Yes | Yes | Poll SEP-1686 task status |
| `GET /v1/tasks/:id/stream` | Yes | Yes | Yes | SSE Server-Sent Events for task completion |
| `GET /health` | Yes | Yes | Yes | Liveness check |
| `GET /ready` | Yes | Yes | Yes | Readiness with belief count |
| `GET /metrics` | Yes | Yes | Yes | Prometheus text format |
| `GET /.well-known/epica-server-card.json` | Yes | Yes | Yes | MCP 2026 Server Card with JSON Schema |
| `GET /.well-known/jwks.json` | Yes | Yes | Yes | JWKS for RS256 verification |

---

## Tasks primitive (SEP-1686)

System 2 (LLM reflection) is natively async. `POST /v1/beliefs` and `PATCH /v1/beliefs/:key` return a `task_id` when System 2 is activated:

```json
{
  "status": "system2_activated",
  "task_id": "task:550e8400-e29b-41d4-a716-446655440000"
}
```

The caller polls `GET /v1/tasks/:id` for the `slow_confidence` result, or subscribes to `GET /v1/tasks/:id/stream` for SSE push.

**Current behavior**: System 2 executes synchronously. The task is created and immediately marked `Completed` in the same request. The polling endpoint returns the result without waiting. This is semantically correct - the client does not observe broken behavior - but the "pending" -> "completed" lifecycle is not exercised under real async load today (TD-P5-002).

---

## Server Card

`/.well-known/epica-server-card.json` is discoverable without a connection. It contains:

- JSON Schema Draft 2020-12 for every endpoint request/response
- OAuth 2.1 metadata block (`authorization_endpoint`, `token_endpoint`, PKCE requirements)
- `capabilities[]` array for agent negotiation (belief revision, contracts, tasks, sovereignty)
- `schema_descriptor` generated from `#[derive(BeliefState)]` via `SchemaDescriptor`

The Server Card is static - it reflects the compiled schema, not runtime state.

---

## Auth

**Development**: set `EPICA_NO_AUTH=1` to bypass JWT validation.

**HS256 (default)**: set `EPICA_JWT_SECRET`. Tokens are validated with `jsonwebtoken` using HMAC-SHA256.

**RS256**: set `EPICA_JWT_ALG=rs256` and either `EPICA_JWT_RSA_PEM` (PEM string) or `EPICA_JWT_RSA_PEM_FILE` (path). JWKS endpoint at `/.well-known/jwks.json` returns the public key in JWK format.

Paths exempt from auth: `/health`, `/ready`, `/metrics`, `/.well-known/*`.

---

## Current status

- All 16 routes: implemented and tested
- 29 E2E tests passing: `cargo test -p epica-mcp`
- Smoke-tested manually (see commands above)

## Known limitations

- Task store is in-memory; tasks lost on restart ([TD-P5-002](phase_roadmap.md#open-technical-debts))
- System 2 is synchronous; "pending" task state not observable in current tests ([TD-P5-002](phase_roadmap.md#open-technical-debts))
- `EPICA_CONTRACTS_FILE` TOML loading resolved (TD-P5-001 - `ContractConfig` implemented)
- No retry or circuit-breaker on System 2 LLM calls
