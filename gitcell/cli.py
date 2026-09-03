"""Command line interface for gitcell."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from . import __version__
from .git_ops import GitError, GitRepo
from .storage import InteractionStore
from .workflow import WorkflowError, WorkflowRunner


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="gitcell",
        description="Local-first git companion: git ops, agent prompt "
        "history, and local Action-style workflows.",
    )
    parser.add_argument("--version", action="version", version=__version__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("init", help="Initialize a git repo and gitcell storage")
    subparsers.add_parser("status", help="Show git status")

    commit_parser = subparsers.add_parser("commit", help="Stage and commit changes")
    commit_parser.add_argument("-m", "--message", required=True)
    commit_parser.add_argument("paths", nargs="*", help="Paths to stage (default: all)")

    log_parser = subparsers.add_parser("log", help="Show recent git commits")
    log_parser.add_argument("-n", "--limit", type=int, default=10)

    prompt_parser = subparsers.add_parser("prompt", help="Manage agent prompt history")
    prompt_sub = prompt_parser.add_subparsers(dest="prompt_command", required=True)

    prompt_log = prompt_sub.add_parser("log", help="Record a prompt/response interaction")
    prompt_log.add_argument("role", help="e.g. user, agent, tool")
    prompt_log.add_argument("content", help="The prompt or response text")

    prompt_list = prompt_sub.add_parser("list", help="List recorded interactions")
    prompt_list.add_argument("-n", "--limit", type=int, default=20)
    prompt_list.add_argument("--role", default=None)

    workflow_parser = subparsers.add_parser("workflow", help="Manage local workflows")
    workflow_sub = workflow_parser.add_subparsers(dest="workflow_command", required=True)
    workflow_sub.add_parser("list", help="List discovered workflows")

    workflow_run = workflow_sub.add_parser("run", help="Run a workflow by name")
    workflow_run.add_argument("name")
    workflow_run.add_argument("--event", default=None)

    return parser


def _cmd_init(args: argparse.Namespace) -> int:
    repo = GitRepo(".")
    repo.init()
    store = InteractionStore(".")
    db_path = store.init()
    print(f"Initialized git repository and gitcell storage at {db_path}")
    return 0


def _cmd_status(args: argparse.Namespace) -> int:
    print(GitRepo(".").status())
    return 0


def _cmd_commit(args: argparse.Namespace) -> int:
    repo = GitRepo(".")
    repo.add(*args.paths)
    result = repo.commit(args.message)
    print(result.stdout)
    return 0


def _cmd_log(args: argparse.Namespace) -> int:
    print(GitRepo(".").log(limit=args.limit))
    return 0


def _cmd_prompt_log(args: argparse.Namespace) -> int:
    store = InteractionStore(".")
    interaction = store.record(args.role, args.content)
    print(f"Recorded interaction #{interaction.id} ({interaction.role})")
    return 0


def _cmd_prompt_list(args: argparse.Namespace) -> int:
    store = InteractionStore(".")
    for interaction in store.list(limit=args.limit, role=args.role):
        print(f"[{interaction.id}] {interaction.timestamp} {interaction.role}: {interaction.content}")
    return 0


def _cmd_workflow_list(args: argparse.Namespace) -> int:
    runner = WorkflowRunner(".")
    workflows = runner.discover()
    if not workflows:
        print("No workflows found in .gitcell/workflows")
        return 0
    for workflow in workflows:
        events = ", ".join(workflow.events) or "any"
        print(f"{workflow.name} (file={workflow.path.name}, on={events})")
    return 0


def _cmd_workflow_run(args: argparse.Namespace) -> int:
    runner = WorkflowRunner(".")
    workflow = runner.find(args.name)
    result = runner.run(workflow, event=args.event)
    for job in result.jobs:
        print(f"job: {job.name}")
        for step in job.steps:
            status = "ok" if step.ok else "FAILED"
            print(f"  step '{step.name}' -> {status}")
            if step.stdout:
                print(step.stdout.rstrip("\n"))
            if not step.ok and step.stderr:
                print(step.stderr.rstrip("\n"), file=sys.stderr)
    return 0 if result.ok else 1


def main(argv: list[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)

    try:
        if args.command == "init":
            return _cmd_init(args)
        if args.command == "status":
            return _cmd_status(args)
        if args.command == "commit":
            return _cmd_commit(args)
        if args.command == "log":
            return _cmd_log(args)
        if args.command == "prompt":
            if args.prompt_command == "log":
                return _cmd_prompt_log(args)
            if args.prompt_command == "list":
                return _cmd_prompt_list(args)
        if args.command == "workflow":
            if args.workflow_command == "list":
                return _cmd_workflow_list(args)
            if args.workflow_command == "run":
                return _cmd_workflow_run(args)
    except (GitError, WorkflowError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    parser.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())
