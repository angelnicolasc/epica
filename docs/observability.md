# Observability

Epica's MCP server exposes three observability surfaces, each tuned to a
different operator workflow.

| Surface | Endpoint / mechanism | What it answers |
|---|---|---|
| Liveness / readiness | `GET /health`, `GET /ready` | Is the process up? Has the runtime loaded? |
| Metrics (Prometheus) | `GET /metrics` (text exposition) | Throughput, error rates, System 2 budget, contract violations |
| Distributed traces (OTLP) | OTLP gRPC export to a collector | Per-request span trees including tower-http, belief handlers, and any spawned System 2 tasks |

The first two ship in the default build. The third is gated behind the
`otlp` feature so the binary stays slim when an OpenTelemetry pipeline is
not in scope.

---

## Prometheus metrics

The recorder is installed at process startup by `install_prometheus()`
and the rendered text format is served by `GET /metrics`. The current
named series:

| Name | Type | Labels |
|---|---|---|
| `epica_belief_inserts_total` | counter | — |
| `epica_belief_updates_total` | counter | `system2={true,false,throttled}` |
| `epica_system2_activations_total` | counter | — |
| `epica_system2_throttled_total` | counter | — |
| `epica_query_duration_seconds` | histogram | — |
| `epica_checkpoint_operations_total` | counter | — |
| `epica_rollback_operations_total` | counter | — |
| `epica_contract_violations_total` | counter | — |

Wire into Grafana / Mimir / Cortex by scraping `http://your-host:8765/metrics`.

---

## OpenTelemetry OTLP (feature `otlp`)

### Build

```bash
cargo build --release --bin epica-serve --features "server anthropic otlp"
```

### Run with a local collector

The simplest local setup is a single-container OpenTelemetry Collector
that forwards to Jaeger:

```yaml
# docker-compose.yml
services:
  jaeger:
    image: jaegertracing/all-in-one:1.60
    ports:
      - "16686:16686"   # Jaeger UI
      - "4317:4317"     # OTLP gRPC
```

```bash
docker compose up -d
EPICA_OTLP_ENDPOINT=http://localhost:4317 \
EPICA_NO_AUTH=1 EPICA_ENV=development \
./target/release/epica-serve
```

Then drive some traffic and visit <http://localhost:16686> — the
`epica-mcp` service appears with one span per HTTP request, nested with
the spans the belief handlers emit (`#[instrument]` on `update_belief`,
`apply_system2_result`, and so on).

### Graceful failure modes

`init_otlp("epica-mcp")` never panics. Three outcomes:

1. **`EPICA_OTLP_ENDPOINT` unset** → log `"OTLP exporter disabled"` and
   continue without OTLP. The Prometheus surface is unaffected.
2. **Collector unreachable** → log a warning when the first batch fails
   to flush; spans drop silently after that. The process keeps serving
   requests.
3. **Subscriber lock already taken** (e.g. another layer beat us to
   `tracing_subscriber::registry().try_init()`) → log a warning, skip
   OTLP attach, continue.

These are intentional: a misconfigured trace pipeline must never take
down a production server.

### Without the feature

If `EPICA_OTLP_ENDPOINT` is set but the binary was built without
`--features otlp`, `init_otlp` logs a warning telling the operator to
rebuild with the feature. No panic, no silent drop.

---

## Why not always-on OTLP

The OpenTelemetry crate stack pulls in tonic + prost + hyper-rustls,
which adds ~2 MB to the release binary and ~30 seconds to a cold
compile. Operators running a single-node deployment with `/metrics` are
unlikely to want any of that. The feature flag is a courtesy, not a
limitation — turn it on in the manifest of any container image you ship
to a Kubernetes cluster.
