use agent_qa_harness::{VERSION, mcp::Harness, store::Store};
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use rmcp::ServiceExt;
use serde_json::json;
use std::{io::Write, path::PathBuf};

#[derive(Parser)]
#[command(name="agent-qa",version=VERSION,about="Native Rust QA MCP runtime")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    Serve {
        #[arg(long, env = "QA_WORKSPACE", default_value = ".")]
        workspace: PathBuf,
        #[arg(long, env = "QA_DATA_DIR")]
        data_dir: Option<PathBuf>,
        #[arg(long,default_value_t=4,value_parser=clap::value_parser!(u16).range(1..=32))]
        max_concurrency: u16,
        #[arg(long,default_value_t=2,value_parser=clap::value_parser!(u16).range(1..=16))]
        max_browsers: u16,
    },
    Report {
        job_id: String,
        #[arg(long)]
        data_dir: PathBuf,
    },
    Config {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long,value_parser=["codex","claude","cursor","antigravity"])]
        client: String,
    },
    Package {
        #[arg(long, default_value = ".")]
        source: PathBuf,
        #[arg(long, default_value = "dist/plugins")]
        output: PathBuf,
    },
}
fn main() -> Result<()> {
    let args = Args::parse();
    let workers = match &args.command {
        Commands::Serve {
            max_concurrency, ..
        } => *max_concurrency as usize,
        _ => 2,
    };
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(workers)
        .enable_all()
        .build()?
        .block_on(run(args))
}
async fn run(args: Args) -> Result<()> {
    match args.command {
        Commands::Serve {
            workspace,
            data_dir,
            max_concurrency,
            max_browsers,
        } => {
            let workspace = workspace.canonicalize()?;
            let store = Store::new(
                &data_dir.unwrap_or_else(|| workspace.join(".qa-harness")),
                &workspace,
            )?;
            let harness = Harness::new(store, max_concurrency as usize, max_browsers as usize)?;
            let service = harness.clone().serve(rmcp::transport::stdio()).await?;
            tokio::select! {result=service.waiting()=>{result?;},_=tokio::signal::ctrl_c()=>{}}
            harness.shutdown().await;
        }
        Commands::Report { job_id, data_dir } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &Store::new(&data_dir, &std::env::current_dir()?)?.export(&job_id)?
                )?
            );
        }
        Commands::Config { workspace, client } => {
            let workspace = workspace.canonicalize()?;
            let executable = std::env::current_exe()?;
            let args = json!(["serve", "--workspace", workspace]);
            if client == "codex" {
                println!(
                    "[mcp_servers.agent-qa]\ncommand = {}\nargs = {}",
                    serde_json::to_string(&executable)?,
                    args
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({"mcpServers":{"agent-qa":{"command":executable,"args":args}}})
                    )?
                );
            }
        }
        Commands::Package { source, output } => {
            let source = source.canonicalize()?;
            std::fs::create_dir_all(&output)?;
            let output = output.canonicalize()?;
            ensure!(
                !source.starts_with(&output) && source != output,
                "Output must not contain the source checkout"
            );
            for (client, manifest, config) in [
                ("codex", ".codex-plugin", ".mcp.json"),
                ("claude", ".claude-plugin", ".mcp.json"),
                ("cursor", ".cursor-plugin", "mcp.json"),
                ("antigravity", ".antigravity-plugin", "mcp_config.json"),
            ] {
                let dest = output.join(client);
                std::fs::create_dir_all(dest.join(manifest))?;
                for name in ["README.md", "README.ja.md", "LICENSE", config] {
                    std::fs::copy(source.join(name), dest.join(name))?;
                }
                std::fs::copy(
                    source.join(manifest).join("plugin.json"),
                    dest.join(manifest).join("plugin.json"),
                )?;
                for name in ["skills", "assets", "docs"] {
                    copy_tree(&source.join(name), &dest.join(name))?;
                }
                writeln!(std::io::stdout(), "{client}: {}", dest.display())?;
            }
        }
    }
    Ok(())
}
fn copy_tree(source: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_tree(&entry.path(), &dest.join(entry.file_name()))?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), dest.join(entry.file_name()))?;
        }
    }
    Ok(())
}
