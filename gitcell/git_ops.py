"""Thin wrappers around common git operations.

gitcell does not reimplement git; it shells out to the system ``git``
binary and adds a bit of convenience (consistent return values, a
``.gitcell`` sidecar directory bootstrap, etc.).
"""
from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path


class GitError(RuntimeError):
    """Raised when the underlying git command fails."""


@dataclass
class GitResult:
    returncode: int
    stdout: str
    stderr: str

    @property
    def ok(self) -> bool:
        return self.returncode == 0


def _run(args: list[str], cwd: Path) -> GitResult:
    proc = subprocess.run(
        ["git", *args],
        cwd=str(cwd),
        capture_output=True,
        text=True,
        check=False,
    )
    return GitResult(proc.returncode, proc.stdout.strip(), proc.stderr.strip())


class GitRepo:
    """A thin, convenience wrapper around a git working tree."""

    def __init__(self, path: str | Path = "."):
        self.path = Path(path).resolve()

    def _exec(self, args: list[str]) -> GitResult:
        result = _run(args, self.path)
        if not result.ok:
            raise GitError(result.stderr or f"git {' '.join(args)} failed")
        return result

    def init(self) -> GitResult:
        self.path.mkdir(parents=True, exist_ok=True)
        return self._exec(["init"])

    def status(self) -> str:
        return self._exec(["status", "--short", "--branch"]).stdout

    def add(self, *paths: str) -> GitResult:
        targets = list(paths) or ["."]
        return self._exec(["add", *targets])

    def commit(self, message: str) -> GitResult:
        return self._exec(["commit", "-m", message])

    def log(self, limit: int = 10) -> str:
        return self._exec(
            ["log", f"-{limit}", "--pretty=format:%h %ad %s", "--date=short"]
        ).stdout

    def current_branch(self) -> str:
        return self._exec(["rev-parse", "--abbrev-ref", "HEAD"]).stdout

    def is_git_repo(self) -> bool:
        result = _run(["rev-parse", "--is-inside-work-tree"], self.path)
        return result.ok and result.stdout == "true"
