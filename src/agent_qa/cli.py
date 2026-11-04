import argparse
import os
from pathlib import Path

from . import __version__
from .store import Store


def main():
    parser = argparse.ArgumentParser(description="Local QA evidence harness for MCP coding agents")
    parser.add_argument("--version", action="version", version=__version__)
    sub = parser.add_subparsers(dest="command", required=True)
    serve = sub.add_parser("serve", help="Run the MCP server over stdio")
    serve.add_argument("--workspace", default=os.environ.get("QA_WORKSPACE", os.getcwd()))
    serve.add_argument("--data-dir", default=os.environ.get("QA_DATA_DIR"))
    report = sub.add_parser("report", help="Export an existing job without a model")
    report.add_argument("job_id")
    report.add_argument("--data-dir", required=True)
    install = sub.add_parser("config", help="Print explicit client MCP config; never edits client settings")
    install.add_argument("--workspace", required=True)
    install.add_argument("--client", choices=["codex", "claude", "cursor", "antigravity"], required=True)
    args = parser.parse_args()
    if args.command == "serve":
        from .server import create_server

        root = Path(args.workspace).resolve(strict=True)
        data = Path(args.data_dir).resolve() if args.data_dir else root / ".qa-harness"
        create_server(data, root).run(transport="stdio")
    elif args.command == "report":
        import json

        print(json.dumps(Store(args.data_dir).export(args.job_id), ensure_ascii=False, indent=2))
    else:
        from .install import configuration

        print(configuration(args.client, Path(args.workspace), Path(__file__).resolve().parents[2]))


if __name__ == "__main__":
    main()
