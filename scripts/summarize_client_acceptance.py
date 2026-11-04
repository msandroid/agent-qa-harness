"""Inspect actual client-produced ledgers; never manufactures client success."""

import argparse
import hashlib
import json
import sqlite3
from pathlib import Path


def summarize(workspace: Path):
    root = workspace.resolve()
    receipt_path = root / "run-receipt.json"
    receipt = json.loads(receipt_path.read_text(encoding="utf-8")) if receipt_path.exists() else {}
    result = {
        "client": receipt.get("client"),
        "exit_code": receipt.get("exit_code"),
        "elapsed_seconds": receipt.get("elapsed_seconds"),
        "workspace": str(root),
        "jobs": [],
    }
    data = root / ".qa-harness"
    database = data / "ledger.sqlite3"
    if not database.exists():
        result["status"] = "no_qa_ledger"
        return result
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as c:
        jobs = [json.loads(row[0]) for row in c.execute("SELECT data FROM jobs")]
        for job in jobs:
            findings = [
                json.loads(row[0]) for row in c.execute("SELECT data FROM findings WHERE job=?", (job["id"],))
            ]
            artifacts = [
                json.loads(row[0])
                for row in c.execute("SELECT data FROM artifacts WHERE job=?", (job["id"],))
            ]
            events = [
                json.loads(row[0])
                for row in c.execute(
                    "SELECT data FROM events WHERE job=? AND kind='step_started'", (job["id"],)
                )
            ]
            integrity = all(
                (data / a["stored_path"]).is_file()
                and hashlib.sha256((data / a["stored_path"]).read_bytes()).hexdigest() == a["sha256"]
                for a in artifacts
            )
            reports = data / "reports" / job["id"]
            result["jobs"].append(
                {
                    "id": job["id"],
                    "status": job["status"],
                    "build_id": job["contract"]["build_id"],
                    "steps": job["steps_used"],
                    "checkpoint": bool(job["checkpoint"]),
                    "browser_actions": sum(e["source"] == "browser_collected" for e in events),
                    "artifact_count": len(artifacts),
                    "artifact_integrity": integrity,
                    "outcomes": [
                        {"criterion_id": o["criterion_id"], "result": o["result"]}
                        for o in job["outcomes"] or []
                    ],
                    "findings": [
                        {
                            "id": f["id"],
                            "status": f["status"],
                            "title": f["title"],
                            "reproductions": len(f["reproductions"]),
                            "separate_run": all(r["run_id"] != f["run_id"] for r in f["reproductions"]),
                        }
                        for f in findings
                    ],
                    "markdown": str(reports / "report.md") if (reports / "report.md").is_file() else None,
                    "json": str(reports / "report.json") if (reports / "report.json").is_file() else None,
                }
            )
    result["status"] = (
        "end_to_end_pass"
        if any(
            j["status"] == "completed"
            and j["checkpoint"]
            and j["browser_actions"] > 0
            and j["artifact_integrity"]
            and j["markdown"]
            and j["json"]
            and any(
                f["status"] == "confirmed" and f["reproductions"] and f["separate_run"] for f in j["findings"]
            )
            for j in result["jobs"]
        )
        else "incomplete"
    )
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("workspaces", nargs="+", type=Path)
    args = parser.parse_args()
    print(json.dumps([summarize(p) for p in args.workspaces], ensure_ascii=False, indent=2))
