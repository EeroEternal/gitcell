# gitcell

A local-first companion for git repositories, inspired by the idea of
using [celld](https://x.com/rough__sea/status/2093443347902038115) style
local execution instead of relying on GitHub's hosted services.

gitcell has three pillars, all running entirely on your machine:

1. **Basic git operations** — thin wrappers around the git CLI
   (`init`, `status`, `commit`, `log`) via `gitcell` commands.
2. **Agent prompt history** — every prompt/response exchanged with an
   AI agent can be recorded locally (SQLite, under `.gitcell/gitcell.db`)
   so you keep a durable, queryable record of how the code changed.
3. **Local workflows** — a minimal, GitHub Actions-like YAML runner
   that executes workflow steps locally (no cloud runners, no
   containers), reading definitions from `.gitcell/workflows/*.yml`.

## Install

```bash
pip install -e .
```

## Usage

```bash
# Initialize git + local gitcell storage
gitcell init

# Basic git operations
gitcell status
gitcell commit -m "message"
gitcell log -n 5

# Record and inspect agent prompt/response history
gitcell prompt log user "please add a login feature"
gitcell prompt log agent "done, see commit abc123"
gitcell prompt list

# Discover and run local Action-style workflows
gitcell workflow list
gitcell workflow run CI --event push
```

Workflow files use a small subset of the GitHub Actions schema:

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
      - name: Show python version
        run: python3 --version
```

See `.gitcell/workflows/ci.yml` for a working example.

## Development

```bash
pip install -e .[dev]  # or: pip install -e . pytest
pytest
```
