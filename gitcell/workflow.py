"""A minimal, local runner for GitHub Actions-like workflows.

Workflows are YAML files under ``.gitcell/workflows/*.yml`` using a
deliberately small subset of the GitHub Actions schema:

    name: CI
    on: [push, manual]
    env:
      GREETING: hello
    jobs:
      build:
        env:
          JOB_VAR: 1
        steps:
          - name: Say hello
            run: echo "$GREETING, $JOB_VAR"
          - name: Run tests
            run: pytest

Everything runs locally via the system shell -- there is no
containerization or remote execution, matching gitcell's "local first"
philosophy.
"""
from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

import yaml

DEFAULT_WORKFLOWS_DIR = ".gitcell/workflows"


class WorkflowError(RuntimeError):
    """Raised when a workflow file is invalid or cannot be found."""


@dataclass
class StepResult:
    name: str
    command: str
    returncode: int
    stdout: str
    stderr: str

    @property
    def ok(self) -> bool:
        return self.returncode == 0


@dataclass
class JobResult:
    name: str
    steps: list[StepResult] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return all(step.ok for step in self.steps)


@dataclass
class WorkflowResult:
    name: str
    jobs: list[JobResult] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return all(job.ok for job in self.jobs)


def _normalize_events(raw_on: Any) -> list[str]:
    if raw_on is None:
        return []
    if isinstance(raw_on, str):
        return [raw_on]
    if isinstance(raw_on, list):
        return [str(e) for e in raw_on]
    if isinstance(raw_on, dict):
        return list(raw_on.keys())
    raise WorkflowError(f"Unsupported 'on' value: {raw_on!r}")


class Workflow:
    def __init__(self, path: Path, spec: dict[str, Any]):
        self.path = path
        self.spec = spec
        self.name = spec.get("name", path.stem)
        # PyYAML (YAML 1.1) parses the bare key `on` as the boolean True,
        # so also check for that in addition to the literal "on" key.
        raw_on = spec.get("on", spec.get(True))
        self.events = _normalize_events(raw_on)
        self.env = spec.get("env", {}) or {}
        self.jobs = spec.get("jobs", {}) or {}

    def matches_event(self, event: Optional[str]) -> bool:
        if event is None or not self.events:
            return True
        return event in self.events

    @classmethod
    def load(cls, path: Path) -> "Workflow":
        with open(path, "r", encoding="utf-8") as f:
            spec = yaml.safe_load(f) or {}
        if not isinstance(spec, dict):
            raise WorkflowError(f"Workflow file {path} must contain a mapping")
        return cls(path, spec)


class WorkflowRunner:
    """Discovers and runs workflows from a local directory."""

    def __init__(self, repo_path: str | Path = "."):
        self.repo_path = Path(repo_path).resolve()
        self.workflows_dir = self.repo_path / DEFAULT_WORKFLOWS_DIR

    def discover(self) -> list[Workflow]:
        if not self.workflows_dir.exists():
            return []
        paths = sorted(
            p
            for ext in ("*.yml", "*.yaml")
            for p in self.workflows_dir.glob(ext)
        )
        return [Workflow.load(p) for p in paths]

    def find(self, name: str) -> Workflow:
        for workflow in self.discover():
            if workflow.name == name or workflow.path.stem == name:
                return workflow
        raise WorkflowError(f"No workflow named {name!r} found in {self.workflows_dir}")

    def run(
        self,
        workflow: Workflow,
        event: Optional[str] = None,
        stop_on_failure: bool = True,
    ) -> WorkflowResult:
        if not workflow.matches_event(event):
            raise WorkflowError(
                f"Workflow {workflow.name!r} does not listen for event {event!r}"
            )

        result = WorkflowResult(name=workflow.name)
        for job_name, job_spec in workflow.jobs.items():
            job_result = self._run_job(workflow, job_name, job_spec or {})
            result.jobs.append(job_result)
            if stop_on_failure and not job_result.ok:
                break
        return result

    def _run_job(
        self, workflow: Workflow, job_name: str, job_spec: dict[str, Any]
    ) -> JobResult:
        job_env = {**workflow.env, **(job_spec.get("env") or {})}
        job_result = JobResult(name=job_name)
        for index, step_spec in enumerate(job_spec.get("steps", []) or []):
            step_result = self._run_step(index, step_spec, job_env)
            job_result.steps.append(step_result)
            if not step_result.ok:
                break
        return job_result

    def _run_step(
        self, index: int, step_spec: dict[str, Any], job_env: dict[str, Any]
    ) -> StepResult:
        command = step_spec.get("run")
        step_name = step_spec.get("name", f"step-{index}")
        if not command:
            raise WorkflowError(f"Step {step_name!r} has no 'run' command")

        env = {**os.environ, **{k: str(v) for k, v in job_env.items()}}
        proc = subprocess.run(
            command,
            shell=True,
            cwd=str(self.repo_path),
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )
        return StepResult(
            name=step_name,
            command=command,
            returncode=proc.returncode,
            stdout=proc.stdout,
            stderr=proc.stderr,
        )
