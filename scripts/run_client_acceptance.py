"""Run an installed coding client against a black-box local QA fixture.

No model APIs are called here: the selected client's existing authentication is used.
Private transcripts and app evidence stay outside the repository in the chosen workspace.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def run(client: str, workspace: Path, timeout: int, interactive: bool = False):
    workspace = workspace.resolve()
    workspace.mkdir(parents=True, exist_ok=True)
    html = (ROOT / "evals/client_acceptance/app.html").read_bytes()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(html)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{server.server_port}"
    prompt = (
        "Validate the actually installed agent-qa plugin end to end. Load its qa-explore skill and use "
        "its MCP tools, not a hand-written replacement. You are testing the Harbor Profile application "
        f"at {url}. Build: harbor-profile-fixture-v1. Scope: display-name editing, saving, cancelling, "
        "and persistence after reload. Do not read the application's source, use shell HTTP clients, "
        "or modify the product. Only use agent-qa browser tools to interact with the target. "
        "Explore from the visible UI and help text; do not assume a defect exists. Create one job with "
        "criteria save-persistence and cancel-edit, exact allowed origin, max_steps=60, max_seconds=900. "
        "Use the current workspace and create fresh isolated browser contexts for discovery and reproduction. "
        "If an actual anomaly is found, create a candidate using browser-collected evidence, then close the "
        "browser, start a separate reproduction run/context, repeat and record fresh evidence. "
        "Checkpoint your exploration, finalize both criteria with evidence and honest gaps, export the "
        "JSON and Markdown reports, then close the owned browser. Use no unrelated MCP servers or agents. "
        "The final answer must include the job ID, finding IDs/statuses, report paths and whether the "
        "installed skill and MCP were actually available. Do not merely describe instructions. "
        "You are authorized to use the plugin tools against this disposable localhost test app."
    )
    (workspace / "acceptance-prompt.txt").write_text(prompt, encoding="utf-8")
    if client == "codex":
        command = [
            shutil.which("codex"),
            "exec",
            "--ephemeral",
            "--skip-git-repo-check",
            "--json",
            "--approve-for-me",
            "-C",
            str(workspace),
            prompt,
        ]
    elif client == "claude":
        command = [
            shutil.which("claude"),
            "-p",
            prompt,
            "--output-format",
            "stream-json",
            "--verbose",
            "--allowedTools",
            "Read,Skill,mcp__plugin_agent-qa_agent-qa__*",
            "--permission-mode",
            "dontAsk",
        ]
    elif client == "antigravity":
        if interactive:
            command = [shutil.which("agy"), "--prompt-interactive", prompt]
        else:
            command = [
                shutil.which("agy"),
                "--print",
                prompt,
                "--output-format",
                "stream-json",
                "--mode",
                "auto",
                "--print-timeout",
                f"{timeout}s",
            ]
    else:
        executable = shutil.which("cursor-agent.cmd") or shutil.which("cursor-agent")
        command = [
            executable,
            "--print",
            prompt,
            "--output-format",
            "stream-json",
            "--trust",
            "--approve-mcps",
            "--workspace",
            str(workspace),
        ]
    if not command[0]:
        raise RuntimeError(f"Client executable not found: {client}")
    env = dict(os.environ, QA_WORKSPACE=str(workspace), QA_DATA_DIR=str(workspace / ".qa-harness"))
    # uv tool's user executable directory may be absent from an already-running IDE's PATH.
    env["PATH"] = str(Path.home() / ".local/bin") + os.pathsep + env.get("PATH", "")
    started = time.time()
    receipt = {
        "client": client,
        "workspace": str(workspace),
        "url": url,
        "started_at": started,
        "plugin_mode": "installed discovery",
        "timeout_seconds": timeout,
    }
    try:
        with (
            (workspace / "transcript.jsonl").open("w", encoding="utf-8") as stdout,
            (workspace / "stderr.log").open("w", encoding="utf-8") as stderr,
        ):
            process = subprocess.Popen(
                command,
                cwd=workspace,
                env=env,
                stdout=None if interactive else stdout,
                stderr=None if interactive else stderr,
            )
            receipt["pid"] = process.pid
            (workspace / "run-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
            try:
                receipt["exit_code"] = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                # This process was created by this script; never terminate unrelated clients.
                if os.name == "nt":
                    subprocess.run(
                        ["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True, check=False
                    )
                else:
                    process.terminate()
                receipt["timed_out"] = True
                receipt["exit_code"] = process.wait(timeout=20)
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
        receipt["elapsed_seconds"] = round(time.time() - started, 2)
        (workspace / "run-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps(receipt, indent=2))
    return 124 if receipt.get("timed_out") else receipt["exit_code"]


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("client", choices=["codex", "claude", "antigravity", "cursor"])
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=600)
    parser.add_argument(
        "--interactive",
        action="store_true",
        help="Use Antigravity's interactive, per-action permission prompts",
    )
    args = parser.parse_args()
    sys.exit(run(args.client, args.workspace, args.timeout, args.interactive))
