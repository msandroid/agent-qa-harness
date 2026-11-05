use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    time::{Duration, timeout},
};

#[tokio::test]
async fn native_stdio_with_legacy_protocol_and_parallel_requests() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-qa"))
        .args(["serve", "--workspace"])
        .arg(dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"compatibility-test","version":"1"}}});
    input
        .write_all(format!("{init}\n").as_bytes())
        .await
        .unwrap();
    let reply: Value = serde_json::from_str(
        &timeout(Duration::from_secs(10), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        reply["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    input
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    for (id, method) in [
        (2, "tools/list"),
        (3, "resources/list"),
        (4, "prompts/list"),
    ] {
        input
            .write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":id,"method":method})).as_bytes())
            .await
            .unwrap();
    }
    let mut seen = std::collections::HashSet::new();
    for _ in 0..3 {
        let reply: Value = serde_json::from_str(
            &timeout(Duration::from_secs(10), output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let id = reply["id"].as_u64().unwrap();
        seen.insert(id);
        match id {
            2 => {
                let tools = reply["result"]["tools"].as_array().unwrap();
                assert_eq!(tools.len(), 18);
                for name in ["qa_list_jobs", "qa_get_job", "qa_get_events"] {
                    assert_eq!(
                        tools.iter().find(|t| t["name"] == name).unwrap()["annotations"]["readOnlyHint"],
                        true
                    );
                }
            }
            3 => assert_eq!(reply["result"]["resources"][0]["uri"], "qa://workflow"),
            4 => assert_eq!(reply["result"]["prompts"][0]["name"], "qa_explore"),
            _ => panic!("unexpected reply"),
        }
    }
    assert_eq!(seen.len(), 3);
    let call = json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"qa_create_job","arguments":{"contract":{"title":"Native MCP","target_kind":"software","build_id":"v1","scope":"Boot","criteria":[{"id":"boot","description":"Starts"}]}}}});
    input
        .write_all(format!("{call}\n").as_bytes())
        .await
        .unwrap();
    let reply: Value = serde_json::from_str(
        &timeout(Duration::from_secs(10), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(
        reply["result"]["structuredContent"]["contract"]["build_id"],
        "v1"
    );
    drop(input);
    assert!(
        timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}
