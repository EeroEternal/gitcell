# gitcell

A self-hosted, multi-client alternative to relying on GitHub's hosted
services — a "GitHub replacement" server that runs entirely on
infrastructure you control. The backend is a Rust service scaffolded from
[console-kit](https://github.com/EeroEternal/console-kit).

gitcell is an **online service**: a single server process can host many
repositories and be accessed concurrently by many clients over HTTP.

## Pillars

1. **Basic git operations** — thin wrappers around the system `git` CLI
   (`init`, `status`, `commit`, `log`), scoped per repository so one
   server can manage many repos.
2. **Agent prompt history** — every prompt/response exchanged with an AI
   agent is recorded as durable, replayable events via the embedded
   [`cellz`](https://crates.io/crates/cellz) event-sourced cell engine
   (one `cellz` cell per repository), keyed by repository, so you keep a
   durable, queryable record of how the code changed.
3. **Local Action-style workflows** — a minimal, GitHub Actions-like YAML
   runner that executes workflow steps on the server host (no
   containerization, no remote cloud runners), reading definitions from
   `<repo>/.gitcell/workflows/*.yml`.

## Architecture

- **Framework**: Rust 2024, [Axum](https://github.com/tokio-rs/axum) 0.8 +
  Tokio + Tower + Tracing, scaffolded from
  [console-kit](https://github.com/EeroEternal/console-kit)'s backend
  starter.
- **Storage**: [`cellz`](https://crates.io/crates/cellz)
  (`cellz = { version = "0.2", default-features = false }`, embedded, no
  HTTP stack of its own) for agent prompt/interaction history — each
  repository maps 1:1 to a `cellz` cell (an isolated, single-writer
  SQLite event log under `GITCELL_CELLS_DIR`), giving gitcell a durable,
  replayable event log for prompts/responses. Each managed git repository
  lives under `GITCELL_DATA_DIR` (default `./data/repos/<repo>`) as a
  plain git working tree.
- **Multi-tenancy**: every API route is scoped by a `{repo}` path segment;
  repository names are validated (`^[A-Za-z0-9_-]+$`) to prevent path
  traversal outside the data directory (this also keeps `cellz` cell IDs
  filesystem-safe, since gitcell uses the repository name as the cell ID).

```
src/
  main.rs      - process entrypoint, wiring config/cellz/router
  lib.rs       - crate root
  config.rs    - environment-driven configuration
  error.rs     - unified error type -> HTTP response mapping
  server.rs    - Axum router and HTTP handlers
  git_ops.rs   - git CLI wrappers, repo path/name validation
  storage.rs   - cellz-backed agent prompt/interaction history
  workflow.rs  - local Action-style workflow discovery & execution
tests/         - integration tests (Axum router via `tower::ServiceExt`)
```

## Configuration

All configuration is via environment variables (with sane local
defaults), so the same binary can be deployed for multiple
clients/environments without rebuilding:

| Variable | Default | Purpose |
| --- | --- | --- |
| `GITCELL_HOST` | `0.0.0.0` | Bind address |
| `GITCELL_PORT` | `8080` | Bind port |
| `GITCELL_DATA_DIR` | `./data/repos` | Root directory holding managed git repositories |
| `GITCELL_CELLS_DIR` | `./data/cells` | Root directory for `cellz` per-repo event logs (prompt history) |
| `GITCELL_CELLS_STORAGE_DIR` | `./data/cells-storage` | Blob storage used by `cellz` for cell snapshots/backups/leases |
| `GITCELL_CELLS_LEASE_TTL_SECS` | `60` | Single-writer lease TTL for `cellz` cells |

## Running

```bash
cargo run
```

## API

```bash
# Initialize a new repository
curl -X POST http://localhost:8080/api/v1/repos/my-repo/init

# Basic git operations
curl http://localhost:8080/api/v1/repos/my-repo/status
curl -X POST http://localhost:8080/api/v1/repos/my-repo/commit \
  -H 'content-type: application/json' \
  -d '{"message": "initial commit"}'
curl "http://localhost:8080/api/v1/repos/my-repo/log?limit=5"

# Record and inspect agent prompt/response history
curl -X POST http://localhost:8080/api/v1/repos/my-repo/prompts \
  -H 'content-type: application/json' \
  -d '{"role": "user", "content": "please add a login feature"}'
curl http://localhost:8080/api/v1/repos/my-repo/prompts

# Discover and run local Action-style workflows
curl http://localhost:8080/api/v1/repos/my-repo/workflows
curl -X POST "http://localhost:8080/api/v1/repos/my-repo/workflows/CI/run?event=push"
```

Workflow files use a small subset of the GitHub Actions schema, read from
`<repo>/.gitcell/workflows/*.yml`; see
[`examples/workflows/ci.yml`](examples/workflows/ci.yml) for a working
example:

```yaml
name: CI
on: [push, manual]
env:
  GREETING: "hello from gitcell"
jobs:
  build:
    steps:
      - name: Greet
        run: echo "$GREETING"
      - name: Show shell
        run: uname -a
```

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
