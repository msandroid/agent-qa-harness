use agent_qa_harness::{browser::chrome_path, mcp::Harness, store::Store};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::{Duration, Instant, sleep},
};

async fn call(h: &Harness, name: &str, a: Value) -> Value {
    h.dispatch(name, a).await.unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Chromium; run cargo test --test browser -- --ignored --nocapture"]
async fn real_browser_parallel_isolated_save_reproduction_and_limits() {
    assert!(chrome_path().is_some(), "Set QA_CHROME");
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(&dir.path().join("data"), dir.path()).unwrap();
    let h = Harness::new(store, 4, 2).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let active = Arc::new(AtomicUsize::new(0));
    let overlap = Arc::new(AtomicUsize::new(0));
    let count = overlap.clone();
    let running = active.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let count = count.clone();
            let running = running.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                let n = stream.read(&mut buffer).await.unwrap();
                let slow = String::from_utf8_lossy(&buffer[..n]).starts_with("GET /slow");
                if slow {
                    let a = running.fetch_add(1, Ordering::SeqCst) + 1;
                    count.fetch_max(a, Ordering::SeqCst);
                    sleep(Duration::from_millis(700)).await;
                    running.fetch_sub(1, Ordering::SeqCst);
                }
                let html = r#"<!doctype html><label>Name<input id="name" aria-label="Name" value="Guest"></label><input id="secret" type="password" value="hunter2"><button id="save" onclick="document.querySelector('#status').textContent='Saved: '+document.querySelector('#name').value">Save</button><p id="status">Not saved</p>"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                    html.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    let create = || json!({"contract":{"title":"Parallel browser QA","target_kind":"web","build_id":"buggy-v1","scope":"Save and reload","criteria":[{"id":"save","description":"Name persists"}],"allowed_origins":[origin],"max_steps":40}});
    let j1 = call(&h, "qa_create_job", create()).await;
    let j2 = call(&h, "qa_create_job", create()).await;
    let r1 = call(
        &h,
        "qa_begin_run",
        json!({"job_id":j1["id"],"kind":"discovery","fixture_description":"Fresh browser context"}),
    )
    .await;
    let r2=call(&h,"qa_begin_run",json!({"job_id":j2["id"],"kind":"discovery","fixture_description":"Another isolated context"})).await;
    call(
        &h,
        "qa_browser_start",
        json!({"job_id":j1["id"],"run_id":r1["id"]}),
    )
    .await;
    call(
        &h,
        "qa_browser_start",
        json!({"job_id":j2["id"],"run_id":r2["id"]}),
    )
    .await;
    let j3 = call(&h, "qa_create_job", create()).await;
    let r3 = call(
        &h,
        "qa_begin_run",
        json!({"job_id":j3["id"],"kind":"discovery","fixture_description":"Fresh"}),
    )
    .await;
    assert!(
        h.dispatch(
            "qa_browser_start",
            json!({"job_id":j3["id"],"run_id":r3["id"]})
        )
        .await
        .is_err()
    );
    let nav = |j: &Value, r: &Value| json!({"job_id":j["id"],"run_id":r["id"],"action":{"kind":"navigate","url":format!("{origin}/slow")}});
    let started = Instant::now();
    let (a, b) = tokio::join!(
        h.dispatch("qa_browser_action", nav(&j1, &r1)),
        h.dispatch("qa_browser_action", nav(&j2, &r2))
    );
    assert_eq!(a.unwrap()["state"], "observed");
    assert_eq!(b.unwrap()["state"], "observed");
    assert!(
        overlap.load(Ordering::SeqCst) >= 2,
        "Independent jobs did not overlap"
    );
    println!(
        "Two delayed navigations overlapped in {:?}; max simultaneous HTTP requests={}",
        started.elapsed(),
        overlap.load(Ordering::SeqCst)
    );
    let action = |r: &Value, a: Value| json!({"job_id":j1["id"],"run_id":r["id"],"action":a});
    let filled = call(
        &h,
        "qa_browser_action",
        action(
            &r1,
            json!({"kind":"fill","selector":"#name","value":" Ada "}),
        ),
    )
    .await;
    assert!(!filled["observation"].to_string().contains("hunter2"));
    assert_eq!(filled["observation"]["controls"][0]["value"], " Ada ");
    let saved = call(
        &h,
        "qa_browser_action",
        action(
            &r1,
            json!({"kind":"click","selector":"button:has-text('Save')"}),
        ),
    )
    .await;
    assert!(
        saved["observation"]["visible_text"]
            .as_str()
            .unwrap()
            .contains("Saved: Ada")
    );
    let other = call(
        &h,
        "qa_browser_action",
        json!({"job_id":j2["id"],"run_id":r2["id"],"action":{"kind":"observe"}}),
    )
    .await;
    assert_eq!(other["observation"]["controls"][0]["value"], "Guest");
    let reloaded = call(
        &h,
        "qa_browser_action",
        action(&r1, json!({"kind":"navigate","url":origin})),
    )
    .await;
    assert!(
        reloaded["observation"]["visible_text"]
            .as_str()
            .unwrap()
            .contains("Not saved")
    );
    let f=call(&h,"qa_add_finding",json!({"job_id":j1["id"],"run_id":r1["id"],"finding":{"title":"Save lost","expected":"Persists","actual":"Reverts","steps":["Fill","Save","Reload"],"severity":"high","criterion_id":"save","artifact_ids":[reloaded["observation"]["observation_artifact_id"]]}})).await;
    call(&h, "qa_browser_close", json!({"job_id":j1["id"]})).await;
    let rr=call(&h,"qa_begin_run",json!({"job_id":j1["id"],"kind":"reproduction","fixture_description":"Closed first context; brand new isolated context"})).await;
    call(
        &h,
        "qa_browser_start",
        json!({"job_id":j1["id"],"run_id":rr["id"]}),
    )
    .await;
    call(
        &h,
        "qa_browser_action",
        action(&rr, json!({"kind":"navigate","url":origin})),
    )
    .await;
    call(
        &h,
        "qa_browser_action",
        action(&rr, json!({"kind":"fill","selector":"#name","value":"Ada"})),
    )
    .await;
    call(
        &h,
        "qa_browser_action",
        action(&rr, json!({"kind":"click","selector":"#save"})),
    )
    .await;
    let evidence = call(
        &h,
        "qa_browser_action",
        action(&rr, json!({"kind":"navigate","url":origin})),
    )
    .await;
    let evidence = evidence["observation"]["observation_artifact_id"].clone();
    assert_eq!(call(&h,"qa_record_reproduction",json!({"job_id":j1["id"],"finding_id":f["id"],"run_id":rr["id"],"reproduced":true,"artifact_ids":[evidence],"note":"Loss reproduced in fresh context"})).await["status"],"confirmed");
    let uncertain = call(
        &h,
        "qa_browser_action",
        action(
            &rr,
            json!({"kind":"click","selector":"#missing","timeout_ms":100}),
        ),
    )
    .await;
    assert_eq!(uncertain["state"], "unknown");
    assert!(
        h.dispatch(
            "qa_browser_action",
            action(&rr, json!({"kind":"navigate","url":"https://example.com/"}))
        )
        .await
        .is_err()
    );
    call(&h,"qa_complete_job",json!({"job_id":j1["id"],"outcomes":[{"criterion_id":"save","result":"fail","reason":"Loss reproduced","artifact_ids":[evidence]}]})).await;
    assert_eq!(
        call(&h, "qa_export_report", json!({"job_id":j1["id"]})).await["job_status"],
        "completed"
    );
    h.shutdown().await;
    server.abort();
}
