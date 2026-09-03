"""Local storage for agent/prompt interaction history.

Every prompt exchanged with an agent (human prompt, agent response,
tool call, etc.) can be recorded locally in a small SQLite database
that lives alongside the repository, under ``.gitcell/gitcell.db``.
This keeps a durable, queryable record of "how the code got here"
that complements the git history itself.
"""
from __future__ import annotations

import json
import sqlite3
from contextlib import closing
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

DEFAULT_DIR_NAME = ".gitcell"
DEFAULT_DB_NAME = "gitcell.db"

_SCHEMA = """
CREATE TABLE IF NOT EXISTS interactions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    metadata TEXT
);
"""


@dataclass
class Interaction:
    role: str
    content: str
    metadata: Optional[dict[str, Any]] = None
    timestamp: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )
    id: Optional[int] = None

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "timestamp": self.timestamp,
            "role": self.role,
            "content": self.content,
            "metadata": self.metadata,
        }


class InteractionStore:
    """SQLite-backed store for agent prompt/response history."""

    def __init__(self, repo_path: str | Path = "."):
        self.repo_path = Path(repo_path).resolve()
        self.gitcell_dir = self.repo_path / DEFAULT_DIR_NAME
        self.db_path = self.gitcell_dir / DEFAULT_DB_NAME

    def init(self) -> Path:
        self.gitcell_dir.mkdir(parents=True, exist_ok=True)
        with closing(sqlite3.connect(self.db_path)) as conn:
            conn.execute(_SCHEMA)
            conn.commit()
        return self.db_path

    def _connect(self) -> sqlite3.Connection:
        if not self.db_path.exists():
            self.init()
        return sqlite3.connect(self.db_path)

    def record(
        self,
        role: str,
        content: str,
        metadata: Optional[dict[str, Any]] = None,
    ) -> Interaction:
        interaction = Interaction(role=role, content=content, metadata=metadata)
        with closing(self._connect()) as conn:
            cur = conn.execute(
                "INSERT INTO interactions (timestamp, role, content, metadata) "
                "VALUES (?, ?, ?, ?)",
                (
                    interaction.timestamp,
                    interaction.role,
                    interaction.content,
                    json.dumps(metadata) if metadata is not None else None,
                ),
            )
            conn.commit()
            interaction.id = cur.lastrowid
        return interaction

    def list(self, limit: int = 20, role: Optional[str] = None) -> list[Interaction]:
        query = "SELECT id, timestamp, role, content, metadata FROM interactions"
        params: list[Any] = []
        if role:
            query += " WHERE role = ?"
            params.append(role)
        query += " ORDER BY id DESC LIMIT ?"
        params.append(limit)

        with closing(self._connect()) as conn:
            rows = conn.execute(query, params).fetchall()

        interactions = [
            Interaction(
                id=row[0],
                timestamp=row[1],
                role=row[2],
                content=row[3],
                metadata=json.loads(row[4]) if row[4] else None,
            )
            for row in rows
        ]
        # rows are fetched newest-first; return oldest-first for readability.
        return list(reversed(interactions))
