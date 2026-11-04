"""SQLite transactions are the source of truth, not an agent's conversation."""

import hashlib
import json
import os
import re
import sqlite3
import time
import uuid
from contextlib import contextmanager
from pathlib import Path

from .models import Checkpoint, Contract, FindingInput, Outcome


class HarnessError(ValueError):
    pass


def uid() -> str:
    return uuid.uuid4().hex


def dump(value) -> str:
    return json.dumps(value, ensure_ascii=False, allow_nan=False)


class Store:
    def __init__(self, root: str | Path):
        self.root = Path(root).resolve()
        self.root.mkdir(parents=True, exist_ok=True)
        self.db = self.root / "ledger.sqlite3"
        with self.connect() as c:
            c.executescript("""
                PRAGMA journal_mode=WAL;
                CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, job TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS artifacts(id TEXT PRIMARY KEY, job TEXT NOT NULL,
                    run TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS findings(id TEXT PRIMARY KEY, job TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,
                    job TEXT NOT NULL, at REAL NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);
            """)

    def connect(self):
        c = sqlite3.connect(self.db, timeout=30)
        c.row_factory = sqlite3.Row
        return c

    @contextmanager
    def transaction(self):
        c = self.connect()
        try:
            c.execute("BEGIN IMMEDIATE")
            yield c
            c.commit()
        except BaseException:
            c.rollback()
            raise
        finally:
            c.close()

    def _get(self, c, table: str, ident: str):
        row = c.execute(f"SELECT data FROM {table} WHERE id=?", (ident,)).fetchone()
        if row is None:
            raise HarnessError(f"Unknown {table}: {ident}")
        return json.loads(row["data"])

    def _put(self, c, table, data):
        c.execute(f"UPDATE {table} SET data=? WHERE id=?", (dump(data), data["id"]))

    def _event(self, c, job, kind, data):
        c.execute(
            "INSERT INTO events(job,at,kind,data) VALUES(?,?,?,?)", (job, time.time(), kind, dump(data))
        )

    def _active(self, job):
        if job["status"] != "active":
            raise HarnessError(f"Job is {job['status']}; resume a paused job before working")

    def _budget(self, job):
        self._active(job)
        contract = job["contract"]
        if time.time() - job["created_at"] >= contract["max_seconds"]:
            raise HarnessError("Wall-clock budget exhausted (includes paused time)")
        if job["steps_used"] >= contract["max_steps"]:
            raise HarnessError("Step budget exhausted")
        if job["reported_cost_usd"] > contract["max_reported_cost_usd"]:
            raise HarnessError("Reported cost budget exhausted")

    def create(self, contract: Contract, workspace: str):
        path = Path(workspace).resolve(strict=True)
        if not path.is_dir():
            raise HarnessError("workspace must be an existing directory")
        data = {
            "id": uid(),
            "contract": contract.model_dump(),
            "workspace": str(path),
            "status": "active",
            "created_at": time.time(),
            "steps_used": 0,
            "reported_cost_usd": 0.0,
            "checkpoint": None,
            "outcomes": None,
        }
        with self.transaction() as c:
            c.execute("INSERT INTO jobs VALUES(?,?)", (data["id"], dump(data)))
            self._event(c, data["id"], "created", {"build_id": contract.build_id})
        return data

    def get(self, job_id, include_events=False):
        with self.connect() as c:
            job = self._get(c, "jobs", job_id)
            for table in ("runs", "artifacts", "findings"):
                job[table] = [
                    json.loads(r["data"])
                    for r in c.execute(f"SELECT data FROM {table} WHERE job=? ORDER BY rowid", (job_id,))
                ]
            job["events"] = (
                [
                    dict(r) | {"data": json.loads(r["data"])}
                    for r in c.execute(
                        "SELECT seq,at,kind,data FROM events WHERE job=? ORDER BY seq", (job_id,)
                    )
                ]
                if include_events
                else []
            )
        return job

    def list_jobs(self):
        with self.connect() as c:
            return [
                {
                    "id": j["id"],
                    "title": j["contract"]["title"],
                    "status": j["status"],
                    "build_id": j["contract"]["build_id"],
                }
                for r in c.execute("SELECT data FROM jobs ORDER BY rowid DESC")
                for j in [json.loads(r["data"])]
            ]

    def transition(self, job_id, status):
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            if (j["status"], status) not in {("active", "paused"), ("paused", "active")}:
                raise HarnessError("Only active→paused or paused→active is allowed")
            j["status"] = status
            self._put(c, "jobs", j)
            self._event(c, job_id, "status", {"status": status})
        return j

    def checkpoint(self, job_id, checkpoint: Checkpoint):
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            if j["status"] == "completed":
                raise HarnessError("Completed jobs are immutable")
            j["checkpoint"] = checkpoint.model_dump()
            self._put(c, "jobs", j)
            self._event(c, job_id, "checkpoint", j["checkpoint"])
        return j["checkpoint"]

    def begin_run(self, job_id, kind, fixture):
        if kind not in {"discovery", "reproduction", "regression"} or not fixture.strip():
            raise HarnessError("Provide run kind and an explicit fixture/reset description")
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            self._budget(j)
            run = {
                "id": uid(),
                "job_id": job_id,
                "kind": kind,
                "fixture": fixture,
                "fixture_source": "operator_attested",
                "created_at": time.time(),
            }
            c.execute("INSERT INTO runs VALUES(?,?,?)", (run["id"], job_id, dump(run)))
            self._event(c, job_id, "run_started", run)
        return run

    def reserve_step(self, job_id, run_id, action, source):
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            self._budget(j)
            r = self._get(c, "runs", run_id)
            if r["job_id"] != job_id:
                raise HarnessError("Run belongs to another job")
            j["steps_used"] += 1
            self._put(c, "jobs", j)
            receipt = {
                "action_id": uid(),
                "run_id": run_id,
                "number": j["steps_used"],
                "action": action,
                "source": source,
                "state": "started",
            }
            self._event(c, job_id, "step_started", receipt)
        return receipt

    def finish_step(self, job_id, receipt, observation, state):
        if state not in {"observed", "unknown", "failed"}:
            raise HarnessError("Invalid step state")
        with self.transaction() as c:
            self._event(c, job_id, "step_finished", receipt | {"state": state, "observation": observation})

    def record_external(self, job_id, run_id, action, observation):
        if not action.strip() or not observation.strip():
            raise HarnessError("Action and observation must be nonempty")
        r = self.reserve_step(job_id, run_id, action, "operator_attested")
        self.finish_step(job_id, r, observation, "observed")
        return r | {"state": "observed", "observation": observation}

    def add_cost(self, job_id, usd, note):
        if not isinstance(usd, (int, float)) or not 0 <= usd <= 100000 or not note.strip():
            raise HarnessError("Provide finite nonnegative USD and a note")
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            if j["status"] == "completed":
                raise HarnessError("Completed jobs are immutable")
            j["reported_cost_usd"] += usd
            self._put(c, "jobs", j)
            self._event(c, job_id, "cost_reported", {"usd": usd, "note": note})
        return {
            "reported_cost_usd": j["reported_cost_usd"],
            "over_budget": j["reported_cost_usd"] > j["contract"]["max_reported_cost_usd"],
        }

    def _check_artifacts(self, c, job_id, ids, run_id=None):
        result = []
        for ident in dict.fromkeys(ids):
            a = self._get(c, "artifacts", ident)
            if a["job_id"] != job_id or (run_id and a["run_id"] != run_id):
                raise HarnessError("Evidence belongs to another job or run")
            path = self.root / a["stored_path"]
            if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != a["sha256"]:
                raise HarnessError("Artifact missing or modified")
            result.append(a)
        return result

    def attach(self, job_id, run_id, source_path, label):
        j = self.get(job_id)
        if j["status"] == "completed":
            raise HarnessError("Completed jobs are immutable")
        path = Path(source_path).resolve(strict=True)
        if not path.is_relative_to(Path(j["workspace"])) or not path.is_file():
            raise HarnessError("Artifact must be a file inside this job's workspace")
        if path.stat().st_size > 25 * 1024 * 1024:
            raise HarnessError("Artifact limit is 25 MiB; split larger captures")
        data = path.read_bytes()
        return self.attach_bytes(job_id, run_id, path.name, data, label, "operator_imported")

    def attach_bytes(self, job_id, run_id, filename, content: bytes, label, source):
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            if j["status"] == "completed":
                raise HarnessError("Completed jobs are immutable")
            r = self._get(c, "runs", run_id)
            if r["job_id"] != job_id:
                raise HarnessError("Run belongs to another job")
            ident = uid()
            # Content is copied. Original paths are never exported.
            suffix = Path(filename).suffix.lower()
            if not re.fullmatch(r"\.[a-z0-9]{1,10}", suffix):
                suffix = ".bin"
            relative = Path("artifacts") / job_id / f"{ident}{suffix}"
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            with destination.open("xb") as f:
                f.write(content)
            a = {
                "id": ident,
                "job_id": job_id,
                "run_id": run_id,
                "label": label,
                "source": source,
                "stored_path": relative.as_posix(),
                "size": len(content),
                "sha256": hashlib.sha256(content).hexdigest(),
                "created_at": time.time(),
            }
            c.execute("INSERT INTO artifacts VALUES(?,?,?,?)", (ident, job_id, run_id, dump(a)))
            self._event(c, job_id, "artifact", a)
        return a

    def finding(self, job_id, run_id, finding: FindingInput):
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            self._active(j)
            if finding.criterion_id not in {x["id"] for x in j["contract"]["criteria"]}:
                raise HarnessError("Unknown criterion")
            self._check_artifacts(c, job_id, finding.artifact_ids, run_id)
            data = finding.model_dump() | {
                "id": uid(),
                "job_id": job_id,
                "run_id": run_id,
                "status": "candidate",
                "reproductions": [],
                "created_at": time.time(),
            }
            c.execute("INSERT INTO findings VALUES(?,?,?)", (data["id"], job_id, dump(data)))
            self._event(c, job_id, "finding", data)
        return data

    def reproduce(self, job_id, finding_id, run_id, reproduced, artifact_ids, note):
        if not note.strip() or not artifact_ids:
            raise HarnessError("Reproduction needs evidence and an observation note")
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            self._active(j)
            f = self._get(c, "findings", finding_id)
            r = self._get(c, "runs", run_id)
            if f["job_id"] != job_id or r["job_id"] != job_id:
                raise HarnessError("Finding/run belongs to another job")
            if r["kind"] != "reproduction" or r["id"] == f["run_id"]:
                raise HarnessError("Use a separate reproduction run after resetting the target")
            if r["created_at"] < f["created_at"]:
                raise HarnessError("Reproduction run predates discovery")
            if any(x["run_id"] == run_id for x in f["reproductions"]):
                raise HarnessError("One reproduction result per run; create a new reset run")
            self._check_artifacts(c, job_id, artifact_ids, run_id)
            f["reproductions"].append(
                {"run_id": run_id, "reproduced": reproduced, "artifact_ids": artifact_ids, "note": note}
            )
            successes = sum(x["reproduced"] for x in f["reproductions"])
            f["status"] = (
                "confirmed"
                if successes == len(f["reproductions"])
                else "intermittent"
                if successes
                else "not_reproduced"
            )
            self._put(c, "findings", f)
            self._event(c, job_id, "reproduction", {"finding_id": finding_id, "result": f})
        return f

    def complete(self, job_id, outcomes: list[Outcome]):
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            if j["status"] == "completed":
                raise HarnessError("Already completed")
            pending = set()
            for row in c.execute("SELECT kind,data FROM events WHERE job=? ORDER BY seq", (job_id,)):
                event = json.loads(row["data"])
                if row["kind"] == "step_started":
                    pending.add(event["action_id"])
                elif row["kind"] == "step_finished":
                    pending.discard(event["action_id"])
            if pending:
                raise HarnessError("Unfinished actions exist; reconcile them with qa_reconcile_step first")
            ids = [o.criterion_id for o in outcomes]
            if len(ids) != len(set(ids)) or set(ids) != {x["id"] for x in j["contract"]["criteria"]}:
                raise HarnessError("Provide exactly one outcome for every criterion")
            for o in outcomes:
                if o.result in {"pass", "fail"} and not o.artifact_ids:
                    raise HarnessError("pass/fail needs evidence; use inconclusive or not_tested otherwise")
                self._check_artifacts(c, job_id, o.artifact_ids)
            findings = [
                json.loads(r["data"]) for r in c.execute("SELECT data FROM findings WHERE job=?", (job_id,))
            ]
            for o in outcomes:
                if o.result == "pass" and any(
                    f["criterion_id"] == o.criterion_id
                    and f["status"] in {"candidate", "confirmed", "intermittent"}
                    for f in findings
                ):
                    raise HarnessError(
                        "A criterion with an unresolved or reproduced finding cannot pass on this build"
                    )
            j["outcomes"] = [o.model_dump() for o in outcomes]
            j["status"] = "completed"
            j["completed_at"] = time.time()
            self._put(c, "jobs", j)
            self._event(c, job_id, "completed", {"outcomes": j["outcomes"]})
        return j

    def reconcile(self, job_id, action_id, observation):
        if not observation.strip():
            raise HarnessError("Reconciliation needs an observation")
        with self.transaction() as c:
            j = self._get(c, "jobs", job_id)
            self._active(j)
            started = None
            finished = False
            for row in c.execute("SELECT kind,data FROM events WHERE job=? ORDER BY seq", (job_id,)):
                e = json.loads(row["data"])
                if e.get("action_id") == action_id:
                    if row["kind"] == "step_started":
                        started = e
                    if row["kind"] == "step_finished":
                        finished = True
            if started is None or finished:
                raise HarnessError("Action is unknown or already finished")
            # Recovery records uncertainty; it never silently replays a side effect.
            self._event(
                c,
                job_id,
                "step_finished",
                started | {"state": "unknown", "observation": observation, "reconciled": True},
            )
        return {"action_id": action_id, "state": "unknown", "observation": observation}

    def export(self, job_id):
        job = self.get(job_id, include_events=True)
        with self.connect() as c:
            self._check_artifacts(c, job_id, [a["id"] for a in job["artifacts"]])
        exported = {k: v for k, v in job.items() if k != "workspace"}
        # Canonical state remains untouched; generated reports are derived artifacts.
        directory = self.root / "reports" / job_id
        directory.mkdir(parents=True, exist_ok=True)
        jp = directory / "report.json"
        jp.write_text(json.dumps(exported, ensure_ascii=False, indent=2), encoding="utf-8")
        lines = [
            f"# {job['contract']['title']}",
            "",
            f"Build: {job['contract']['build_id']}",
            f"Job status: {job['status']}",
            "",
            "Evidence-backed operator assessment; not a guarantee of defect absence.",
            "External observations and fixture resets are operator-attested, not independently verified.",
            "",
            f"Steps: {job['steps_used']} / {job['contract']['max_steps']}",
            f"Reported cost only (not total host billing): ${job['reported_cost_usd']:.4f}",
            "",
            "## Criteria",
            "",
        ]
        for o in job["outcomes"] or []:
            lines += [
                f"- {o['criterion_id']}: **{o['result']}** — {o['reason']}",
                f"  Evidence IDs: {', '.join(o['artifact_ids']) or 'none'}",
            ]
        if job["outcomes"] is None:
            lines += ["No final outcomes recorded; this is an unfinished job."]
        lines += ["", "## Findings", ""]
        for f in job["findings"]:
            lines += [
                f"### {f['title']}",
                "",
                f"Severity: {f['severity']}; status: {f['status']}",
                f"Expected: {f['expected']}",
                f"Actual: {f['actual']}",
                "",
            ]
            lines += [f"{n}. {s}" for n, s in enumerate(f["steps"], 1)]
            successes = sum(r["reproduced"] for r in f["reproductions"])
            lines += [
                "",
                f"Reproductions: {successes}/{len(f['reproductions'])}",
                f"Discovery evidence IDs: {', '.join(f['artifact_ids'])}",
                "",
            ]
        lines += ["## Artifact manifest", ""]
        for a in job["artifacts"]:
            relative = os.path.relpath(self.root / a["stored_path"], directory).replace(os.sep, "/")
            lines += [f"- [{a['id']}]({relative}) — {a['source']}; SHA-256 `{a['sha256']}`"]
        mp = directory / "report.md"
        mp.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return {"json": str(jp), "markdown": str(mp), "job_status": job["status"]}
