#!/usr/bin/env python3
"""Check SQLite locking and shared-library visibility with a second process."""
from service_client import Service
import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="overtalk-database-locks-") as temp:
        root = Path(temp)
        service = Service(args.binary.resolve(), root)
        database = root / "data/com.judeam.sonefold/meeting_minutes.sqlite"
        try:
            project = service.call("workspace_project", action={
                "op": "save", "name": "Synthetic shared library", "context": ""})["id"]
            for i in range(5):
                with closing(sqlite3.connect(database)) as db, db:
                    db.execute("INSERT INTO project_facts(id,project_id,body,created_at,updated_at) "
                               "VALUES(?,?,?,'now','now')", (f"external-{i}", project, "External fact"))
                page = service.call("workspace_project", action={"op": "get", "id": project})
                assert len(page["facts"]) == i * 2 + 1, "Service missed a committed external write"
                service.call("workspace_project", action={
                    "op": "fact", "project_id": project, "body": f"Service fact {i}"})
                with closing(sqlite3.connect(database)) as db:
                    assert db.execute("SELECT count(*) FROM project_facts").fetchone()[0] == (i + 1) * 2, "External reader missed a committed service write"
            if os.name == "posix":
                for suffix in ["", "-wal", "-shm"]:
                    path = Path(str(database) + suffix)
                    if path.exists():
                        assert path.stat().st_mode & 0o077 == 0, "Database or journal permissions are public"
            service.close()
            service = Service(args.binary.resolve(), root)
            page = service.call("workspace_project", action={"op": "get", "id": project})
            assert len(page["facts"]) == 10, "Committed writes were lost after restart"
            with closing(sqlite3.connect(database)) as db:
                assert db.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
                assert db.execute("PRAGMA foreign_key_check").fetchall() == []
        finally:
            service.close()
    report = dict(external_writes_visible="passed", service_writes_visible="passed",
                  restart_preserves_both_writers="passed", integrity="passed")
    if os.name == "posix":
        report["private_database_and_sidecars"] = "passed"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
