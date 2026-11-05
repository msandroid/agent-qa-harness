use agent_qa_harness::{
    mcp::Harness,
    store::{Store, now},
};
use serde_json::{Value, json};
use std::{fs, sync::Arc};

fn setup() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::new(&d.path().join("data"), d.path()).unwrap();
    (d, s)
}
fn create(s: &Store, max_steps: u64) -> Value {
    s.execute("qa_create_job",&json!({"contract":{"title":"Save test","target_kind":"web","build_id":"fixture-v1","scope":"Save and reload","criteria":[{"id":"save","description":"Saved data persists"}],"max_steps":max_steps,"max_seconds":3600,"max_reported_cost_usd":10.,"allowed_origins":[]}})).unwrap()
}
fn run(s: &Store, j: &Value, kind: &str) -> Value {
    s.execute(
        "qa_begin_run",
        &json!({"job_id":j["id"],"kind":kind,"fixture_description":"Fresh fixture"}),
    )
    .unwrap()
}
fn evidence(s: &Store, j: &Value, r: &Value) -> Value {
    s.attach_bytes(
        j["id"].as_str().unwrap(),
        r["id"].as_str().unwrap(),
        "proof.txt",
        b"observed value lost",
        "observation",
    )
    .unwrap()
}
fn finding(s: &Store, j: &Value, r: &Value, a: &Value) -> Value {
    s.execute("qa_add_finding",&json!({"job_id":j["id"],"run_id":r["id"],"finding":{"title":"Saved value lost","expected":"Persists","actual":"Reverts","steps":["Save","Reload"],"severity":"high","criterion_id":"save","artifact_ids":[a["id"]]}})).unwrap()
}
fn outcome(s: &Store, j: &Value, result: &str, ids: Value) -> anyhow::Result<Value> {
    s.execute("qa_complete_job",&json!({"job_id":j["id"],"outcomes":[{"criterion_id":"save","result":result,"reason":"Observed result","artifact_ids":ids}]}))
}

#[test]
fn restart_reproduction_and_streamed_export() {
    let (_d, s) = setup();
    let j = create(&s, 20);
    let r = run(&s, &j, "discovery");
    let a = evidence(&s, &j, &r);
    let f = finding(&s, &j, &r, &a);
    s.execute("qa_checkpoint",&json!({"job_id":j["id"],"checkpoint":{"summary":"Found issue","next_actions":["Reproduce"]}})).unwrap();
    let s = Store::new(&s.root, &s.workspace).unwrap();
    assert_eq!(
        s.basic(j["id"].as_str().unwrap()).unwrap()["checkpoint"]["summary"],
        "Found issue"
    );
    let rr = run(&s, &j, "reproduction");
    let b = evidence(&s, &j, &rr);
    let f=s.execute("qa_record_reproduction",&json!({"job_id":j["id"],"finding_id":f["id"],"run_id":rr["id"],"reproduced":true,"artifact_ids":[b["id"]],"note":"Reproduced after reset"})).unwrap();
    assert_eq!(f["status"], "confirmed");
    assert!(outcome(&s, &j, "pass", json!([b["id"]])).is_err());
    outcome(&s, &j, "fail", json!([b["id"]])).unwrap();
    let paths = s.export(j["id"].as_str().unwrap()).unwrap();
    let report: Value =
        serde_json::from_slice(&fs::read(paths["json"].as_str().unwrap()).unwrap()).unwrap();
    assert!(report.get("workspace").is_none());
    assert_eq!(report["findings"][0]["status"], "confirmed");
    assert!(report["events"][0].get("seq").is_some());
    assert_eq!(report["events"][0]["kind"], "created");
    assert!(s.export(j["id"].as_str().unwrap()).is_ok());
}
#[test]
fn concurrent_atomic_budget() {
    let (_d, s) = setup();
    let j = create(&s, 3);
    let r = run(&s, &j, "discovery");
    let s = Arc::new(s);
    let threads=(0..8).map(|_|{let s=s.clone();let j=j.clone();let r=r.clone();std::thread::spawn(move ||(0..8).filter(|_|s.execute("qa_record_step",&json!({"job_id":j["id"],"run_id":r["id"],"action":"Inspect","observation":"Observed"})).is_ok()).count())}).collect::<Vec<_>>();
    let total: usize = threads.into_iter().map(|t| t.join().unwrap()).sum();
    assert_eq!(total, 3);
    assert_eq!(s.basic(j["id"].as_str().unwrap()).unwrap()["steps_used"], 3);
    let a = evidence(&s, &j, &r);
    finding(&s, &j, &r, &a);
    outcome(&s, &j, "inconclusive", json!([])).unwrap();
}
#[test]
fn cross_job_and_run_evidence_rejected() {
    let (_d, s) = setup();
    let j = create(&s, 20);
    let other = create(&s, 20);
    let r = run(&s, &j, "discovery");
    let rr = run(&s, &other, "discovery");
    let a = evidence(&s, &other, &rr);
    assert!(s.execute("qa_add_finding",&json!({"job_id":j["id"],"run_id":r["id"],"finding":{"title":"Wrong evidence","criterion_id":"save","artifact_ids":[a["id"]]}})).is_err());
    assert!(
        s.reserve(
            j["id"].as_str().unwrap(),
            rr["id"].as_str().unwrap(),
            json!("Inspect"),
            "operator_attested"
        )
        .is_err()
    );
}
#[test]
fn same_run_and_duplicate_reproduction_rejected() {
    let (_d, s) = setup();
    let j = create(&s, 20);
    let r = run(&s, &j, "discovery");
    let a = evidence(&s, &j, &r);
    let f = finding(&s, &j, &r, &a);
    let args = json!({"job_id":j["id"],"finding_id":f["id"],"run_id":r["id"],"reproduced":true,"artifact_ids":[a["id"]],"note":"Attempt"});
    assert!(s.execute("qa_record_reproduction", &args).is_err());
    let rr = run(&s, &j, "reproduction");
    let mut args = args;
    args["run_id"] = rr["id"].clone();
    assert!(s.execute("qa_record_reproduction", &args).is_err());
    let b = evidence(&s, &j, &rr);
    args["artifact_ids"] = json!([b["id"]]);
    s.execute("qa_record_reproduction", &args).unwrap();
    assert!(s.execute("qa_record_reproduction", &args).is_err());
    let rr2 = run(&s, &j, "reproduction");
    let c = evidence(&s, &j, &rr2);
    args["run_id"] = rr2["id"].clone();
    args["artifact_ids"] = json!([c["id"]]);
    args["reproduced"] = json!(false);
    assert_eq!(
        s.execute("qa_record_reproduction", &args).unwrap()["status"],
        "intermittent"
    );
}
#[test]
fn artifact_copy_integrity_and_workspace_boundary() {
    let (d, s) = setup();
    let j = create(&s, 20);
    let r = run(&s, &j, "discovery");
    let input = d.path().join("input.txt");
    fs::write(&input, "original").unwrap();
    let a = s
        .execute(
            "qa_attach_artifact",
            &json!({"job_id":j["id"],"run_id":r["id"],"path":input,"label":"proof"}),
        )
        .unwrap();
    fs::write(input, "changed").unwrap();
    let stored = s.root.join(a["stored_path"].as_str().unwrap());
    assert_eq!(fs::read_to_string(&stored).unwrap(), "original");
    fs::write(stored, "tampered").unwrap();
    assert!(s.export(j["id"].as_str().unwrap()).is_err());
    let external = tempfile::NamedTempFile::new().unwrap();
    assert!(
        s.execute(
            "qa_attach_artifact",
            &json!({"job_id":j["id"],"run_id":r["id"],"path":external.path(),"label":"outside"})
        )
        .is_err()
    );
}
#[test]
fn interrupted_actions_must_be_reconciled() {
    let (_d, s) = setup();
    let j = create(&s, 20);
    let r = run(&s, &j, "discovery");
    let step = s
        .reserve(
            j["id"].as_str().unwrap(),
            r["id"].as_str().unwrap(),
            json!("Submit"),
            "browser_collected",
        )
        .unwrap();
    assert!(outcome(&s, &j, "inconclusive", json!([])).is_err());
    s.execute("qa_reconcile_step",&json!({"job_id":j["id"],"action_id":step["action_id"],"observation":"Inspected after restart; still uncertain"})).unwrap();
    outcome(&s, &j, "inconclusive", json!([])).unwrap();
    assert_eq!(s.basic(j["id"].as_str().unwrap()).unwrap()["steps_used"], 1);
}
#[test]
fn completion_requires_evidence_and_does_not_hide_candidates() {
    let (_d, s) = setup();
    let j = create(&s, 20);
    assert!(outcome(&s, &j, "pass", json!([])).is_err());
    assert!(
        s.execute("qa_complete_job", &json!({"job_id":j["id"],"outcomes":[]}))
            .is_err()
    );
    let r = run(&s, &j, "discovery");
    let a = evidence(&s, &j, &r);
    finding(&s, &j, &r, &a);
    assert!(outcome(&s, &j, "pass", json!([a["id"]])).is_err());
    outcome(&s, &j, "inconclusive", json!([])).unwrap();
    assert!(
        s.attach_bytes(
            j["id"].as_str().unwrap(),
            r["id"].as_str().unwrap(),
            "late.txt",
            b"late",
            "late"
        )
        .is_err()
    );
}
#[test]
fn pause_cost_and_expired_job_boundaries() {
    let (_d, s) = setup();
    let j = create(&s, 20);
    s.execute(
        "qa_set_status",
        &json!({"job_id":j["id"],"status":"paused"}),
    )
    .unwrap();
    assert!(
        s.execute(
            "qa_begin_run",
            &json!({"job_id":j["id"],"kind":"discovery","fixture_description":"Fresh"})
        )
        .is_err()
    );
    s.execute(
        "qa_set_status",
        &json!({"job_id":j["id"],"status":"active"}),
    )
    .unwrap();
    assert!(
        s.execute(
            "qa_report_cost",
            &json!({"job_id":j["id"],"usd":-1,"note":"Invalid"})
        )
        .is_err()
    );
    s.execute(
        "qa_report_cost",
        &json!({"job_id":j["id"],"usd":11,"note":"Measured"}),
    )
    .unwrap();
    assert!(
        s.execute(
            "qa_begin_run",
            &json!({"job_id":j["id"],"kind":"discovery","fixture_description":"Fresh"})
        )
        .is_err()
    );
    let mut j = s.basic(j["id"].as_str().unwrap()).unwrap();
    j["created_at"] = json!(now() - 4000.);
    s.connect()
        .unwrap()
        .execute(
            "UPDATE jobs SET data=?1 WHERE id=?2",
            rusqlite::params![j.to_string(), j["id"].as_str().unwrap()],
        )
        .unwrap();
    assert!(
        s.execute(
            "qa_begin_run",
            &json!({"job_id":j["id"],"kind":"discovery","fixture_description":"Fresh"})
        )
        .unwrap_err()
        .to_string()
        .contains("Wall-clock")
    );
}
#[tokio::test]
async fn mcp_schema_validation_and_origin_normalization() {
    let (_d, s) = setup();
    let h = Harness::new(s, 4, 2).unwrap();
    let mut args = json!({"contract":{"title":"Schema","target_kind":"web","build_id":"v1","scope":"Flow","criteria":[{"id":"save","description":"Persists"}],"allowed_origins":["https://EXAMPLE.com:443/"]}});
    let j = h.dispatch("qa_create_job", args.clone()).await.unwrap();
    assert_eq!(
        j["contract"]["allowed_origins"],
        json!(["https://example.com"])
    );
    args["contract"]["criteria"] =
        json!([{"id":"save","description":"Same"},{"id":"save","description":"Duplicate"}]);
    assert!(h.dispatch("qa_create_job", args.clone()).await.is_err());
    args["contract"]["criteria"] = json!([{"id":"bad id","description":"Invalid"}]);
    assert!(h.dispatch("qa_create_job", args.clone()).await.is_err());
    args["contract"]["criteria"] = json!([{"id":"save","description":"Valid"}]);
    args["contract"]["allowed_origins"] = json!(["https://example.com/path"]);
    assert!(h.dispatch("qa_create_job", args.clone()).await.is_err());
    args["contract"]["allowed_origins"] = json!(["file:///etc/passwd"]);
    assert!(h.dispatch("qa_create_job", args).await.is_err());
}
#[test]
fn reopens_compatible_schema_without_rewriting_records() {
    let (_d, s) = setup();
    let j = create(&s, 20);
    let r = run(&s, &j, "discovery");
    let a = evidence(&s, &j, &r);
    finding(&s, &j, &r, &a);
    let before = s
        .connect()
        .unwrap()
        .query_row(
            "SELECT data FROM jobs WHERE id=?1",
            [j["id"].as_str().unwrap()],
            |r| r.get::<_, String>(0),
        )
        .unwrap();
    let reopened = Store::new(&s.root, &s.workspace).unwrap();
    reopened.export(j["id"].as_str().unwrap()).unwrap();
    let after = reopened
        .connect()
        .unwrap()
        .query_row(
            "SELECT data FROM jobs WHERE id=?1",
            [j["id"].as_str().unwrap()],
            |r| r.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(before, after);
    assert!(
        reopened
            .events(j["id"].as_str().unwrap(), 0, 100)
            .unwrap()
            .as_array()
            .unwrap()
            .len()
            > 1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_caller_retains_capacity_until_committed() {
    let (_d, s) = setup();
    let mut c = s.connect().unwrap();
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let h = Harness::new(s.clone(), 1, 1).unwrap();
    let worker = h.clone();
    let args = json!({"contract":{"title":"Cancelled caller","target_kind":"software","build_id":"v1","scope":"start","criteria":[{"id":"boot","description":"Starts"}]}});
    let task = tokio::spawn(async move { worker.dispatch("qa_create_job", args).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    task.abort();
    assert!(
        h.dispatch("qa_list_jobs", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("busy")
    );
    tx.commit().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), h.shutdown())
        .await
        .unwrap();
    assert_eq!(
        s.execute("qa_list_jobs", &json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
