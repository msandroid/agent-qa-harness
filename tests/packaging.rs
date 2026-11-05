use std::process::Command;

#[test]
fn native_packaging_and_explicit_client_configs() {
    let out = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-qa");
    assert!(
        Command::new(binary)
            .args([
                "package",
                "--source",
                env!("CARGO_MANIFEST_DIR"),
                "--output"
            ])
            .arg(out.path())
            .status()
            .unwrap()
            .success()
    );
    for (client, manifest, config) in [
        ("codex", ".codex-plugin", ".mcp.json"),
        ("claude", ".claude-plugin", ".mcp.json"),
        ("cursor", ".cursor-plugin", "mcp.json"),
        ("antigravity", ".antigravity-plugin", "mcp_config.json"),
    ] {
        let root = out.path().join(client);
        let metadata: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join(manifest).join("plugin.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["version"], env!("CARGO_PKG_VERSION"));
        let server: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join(config)).unwrap()).unwrap();
        assert_eq!(server["mcpServers"]["agent-qa"]["command"], "agent-qa");
        for skill in [
            "qa-explore",
            "qa-reproduce",
            "qa-adversarial-review",
            "qa-resume-report",
        ] {
            assert!(root.join("skills").join(skill).join("SKILL.md").is_file());
        }
        let config = Command::new(binary)
            .args(["config", "--client", client, "--workspace"])
            .arg(out.path())
            .output()
            .unwrap();
        assert!(config.status.success());
        let config = String::from_utf8(config.stdout).unwrap();
        assert!(config.contains("agent-qa"));
        assert!(config.contains("serve"));
    }
}
