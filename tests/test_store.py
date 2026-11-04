import json
from concurrent.futures import ThreadPoolExecutor

import pytest
from pydantic import ValidationError

from agent_qa.models import Checkpoint, Contract, FindingInput, Outcome
from agent_qa.store import HarnessError, Store


def artifact(store, job, run, text=b"observed: save lost"):
    return store.attach_bytes(
        job["id"], run["id"], "observation.txt", text, "observation", "operator_imported"
    )


def finding(store, job, run, evidence):
    return store.finding(
        job["id"],
        run["id"],
        FindingInput(
            title="Saved value lost",
            expected="Persists",
            actual="Lost",
            steps=["Save", "Reload"],
            severity="high",
            criterion_id="save",
            artifact_ids=[evidence["id"]],
        ),
    )


def test_restart_and_report_reproduction(setup):
    store, job, run, _ = setup
    store.record_external(job["id"], run["id"], "Save and reload", "Value reverted")
    a = artifact(store, job, run)
    f = finding(store, job, run, a)
    store.checkpoint(
        job["id"],
        Checkpoint(
            summary="Found save issue",
            hypotheses=["Missing persistence"],
            next_actions=["Reset and reproduce"],
        ),
    )
    restarted = Store(store.root)
    assert restarted.get(job["id"])["checkpoint"]["hypotheses"] == ["Missing persistence"]
    rerun = restarted.begin_run(job["id"], "reproduction", "New profile and fresh fixture")
    b = artifact(restarted, job, rerun)
    result = restarted.reproduce(job["id"], f["id"], rerun["id"], True, [b["id"]], "Same loss after reload")
    assert result["status"] == "confirmed"
    with pytest.raises(HarnessError, match="cannot pass"):
        restarted.complete(
            job["id"],
            [Outcome(criterion_id="save", result="pass", reason="Claimed OK", artifact_ids=[b["id"]])],
        )
    restarted.complete(
        job["id"],
        [Outcome(criterion_id="save", result="fail", reason="Saved data lost", artifact_ids=[b["id"]])],
    )
    paths = restarted.export(job["id"])
    report = json.loads(__import__("pathlib").Path(paths["json"]).read_text(encoding="utf-8"))
    assert "workspace" not in report
    assert report["findings"][0]["status"] == "confirmed"
    assert report["outcomes"][0]["result"] == "fail"


def test_atomic_step_budget_with_concurrent_clients(tmp_path, contract):
    contract.max_steps = 3
    store = Store(tmp_path / "data")
    job = store.create(contract, str(tmp_path))
    run = store.begin_run(job["id"], "discovery", "Fresh target")

    def attempt(_):
        try:
            store.record_external(job["id"], run["id"], "Inspect", "Observed")
            return True
        except HarnessError:
            return False

    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(attempt, range(20)))
    assert sum(results) == 3
    assert store.get(job["id"])["steps_used"] == 3
    # Exhaustion must not prevent preserving evidence and findings or closing with gaps.
    a = artifact(store, job, run)
    finding(store, job, run, a)
    store.checkpoint(job["id"], Checkpoint(summary="Budget reached"))
    store.complete(job["id"], [Outcome(criterion_id="save", result="inconclusive", reason="Budget reached")])


def test_cross_job_evidence_and_run_rejected(setup, contract):
    store, job, run, workspace = setup
    other = store.create(contract, str(workspace))
    other_run = store.begin_run(other["id"], "discovery", "Other target")
    a = artifact(store, other, other_run)
    with pytest.raises(HarnessError, match="another job"):
        finding(store, job, run, a)
    with pytest.raises(HarnessError, match="another job"):
        store.record_external(job["id"], other_run["id"], "Inspect", "Observed")


def test_reproduction_requires_new_evidence_and_unique_run(setup):
    store, job, run, _ = setup
    a = artifact(store, job, run)
    f = finding(store, job, run, a)
    with pytest.raises(HarnessError, match="separate reproduction"):
        store.reproduce(job["id"], f["id"], run["id"], True, [a["id"]], "Same run")
    rerun = store.begin_run(job["id"], "reproduction", "Reset")
    with pytest.raises(HarnessError, match="another job or run"):
        store.reproduce(job["id"], f["id"], rerun["id"], True, [a["id"]], "Old evidence")
    b = artifact(store, job, rerun)
    store.reproduce(job["id"], f["id"], rerun["id"], True, [b["id"]], "Observed")
    with pytest.raises(HarnessError, match="One reproduction"):
        store.reproduce(job["id"], f["id"], rerun["id"], True, [b["id"]], "Duplicate")
    again = store.begin_run(job["id"], "reproduction", "Reset again")
    c = artifact(store, job, again, b"not reproduced")
    result = store.reproduce(job["id"], f["id"], again["id"], False, [c["id"]], "Could not reproduce")
    assert result["status"] == "intermittent"


def test_artifact_import_confined_and_hash_verified(setup, tmp_path):
    store, job, run, workspace = setup
    outside = tmp_path / "outside.txt"
    outside.write_text("private")
    with pytest.raises(HarnessError, match="inside"):
        store.attach(job["id"], run["id"], str(outside), "not allowed")
    inside = workspace / "evidence.txt"
    inside.write_text("original")
    a = store.attach(job["id"], run["id"], str(inside), "allowed")
    inside.write_text("changed original")
    assert (store.root / a["stored_path"]).read_text() == "original"
    (store.root / a["stored_path"]).write_text("tampered copy")
    with pytest.raises(HarnessError, match="modified"):
        store.export(job["id"])


def test_complete_requires_all_criteria_and_evidence(setup):
    store, job, run, _ = setup
    with pytest.raises(HarnessError, match="every criterion"):
        store.complete(job["id"], [])
    with pytest.raises(HarnessError, match="needs evidence"):
        store.complete(job["id"], [Outcome(criterion_id="save", result="pass", reason="No evidence")])
    store.complete(job["id"], [Outcome(criterion_id="save", result="not_tested", reason="No access")])
    with pytest.raises(HarnessError, match="immutable"):
        artifact(store, job, run)


def test_unresolved_candidate_cannot_be_hidden_by_passing_outcome(setup):
    store, job, run, _ = setup
    a = artifact(store, job, run)
    finding(store, job, run, a)
    with pytest.raises(HarnessError, match="unresolved"):
        store.complete(
            job["id"],
            [Outcome(criterion_id="save", result="pass", reason="Claimed OK", artifact_ids=[a["id"]])],
        )


def test_reproduction_run_cannot_be_preallocated_before_finding(setup):
    store, job, run, _ = setup
    earlier = store.begin_run(job["id"], "reproduction", "Premature reset claim")
    a = artifact(store, job, run)
    f = finding(store, job, run, a)
    b = artifact(store, job, earlier)
    with pytest.raises(HarnessError, match="predates"):
        store.reproduce(job["id"], f["id"], earlier["id"], True, [b["id"]], "Claimed reproduction")


def test_recovery_reconciles_inflight_without_replaying(setup):
    store, job, run, _ = setup
    r = store.reserve_step(job["id"], run["id"], "Submit", "browser_collected")
    outcomes = [Outcome(criterion_id="save", result="inconclusive", reason="Runner stopped")]
    with pytest.raises(HarnessError, match="Unfinished"):
        store.complete(job["id"], outcomes)
    restarted = Store(store.root)
    result = restarted.reconcile(job["id"], r["action_id"], "Inspected after restart; result uncertain")
    assert result["state"] == "unknown"
    assert restarted.get(job["id"])["steps_used"] == 1
    restarted.complete(job["id"], outcomes)


def test_pause_cost_and_time_budget(setup, monkeypatch):
    store, job, run, _ = setup
    store.transition(job["id"], "paused")
    with pytest.raises(HarnessError, match="paused"):
        store.record_external(job["id"], run["id"], "Inspect", "Observed")
    store.transition(job["id"], "active")
    store.add_cost(job["id"], 11, "Measured worker charge")
    with pytest.raises(HarnessError, match="cost budget"):
        store.record_external(job["id"], run["id"], "Inspect", "Observed")
    monkeypatch.setattr("agent_qa.store.time.time", lambda: job["created_at"] + 4000)
    with pytest.raises(HarnessError, match="Wall-clock"):
        store.begin_run(job["id"], "reproduction", "Reset")


@pytest.mark.parametrize("value", [float("nan"), float("inf"), -1])
def test_invalid_cost_rejected(setup, value):
    store, job, _, _ = setup
    with pytest.raises(HarnessError):
        store.add_cost(job["id"], value, "Invalid")


def test_contract_validation(contract):
    with pytest.raises(ValidationError):
        Contract.model_validate(contract.model_dump() | {"criteria": [contract.criteria[0]] * 2})
    with pytest.raises(ValidationError):
        Contract.model_validate(contract.model_dump() | {"allowed_origins": ["https://example.com/path"]})
    normalized = Contract.model_validate(
        contract.model_dump() | {"allowed_origins": ["https://EXAMPLE.com:443/"]}
    )
    assert normalized.allowed_origins == ["https://example.com"]
