import json
import os
import sys
from pathlib import Path

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


async def test_real_stdio_handshake_tools_resource_and_workflow(tmp_path):
    project = Path(__file__).resolve().parents[1]
    params = StdioServerParameters(
        command=sys.executable,
        args=["-m", "agent_qa.cli", "serve", "--workspace", str(tmp_path)],
        env=dict(os.environ, PYTHONPATH=str(project / "src")),
    )
    async with stdio_client(params) as (reader, writer):
        async with ClientSession(reader, writer) as client:
            await client.initialize()
            tools = await client.list_tools()
            by_name = {t.name: t for t in tools.tools}
            for name in ("qa_list_jobs", "qa_get_job", "qa_get_events"):
                assert by_name[name].annotations.readOnlyHint is True
            assert (
                by_name["qa_create_job"].annotations is None
                or not by_name["qa_create_job"].annotations.readOnlyHint
            )
            assert {"qa_create_job", "qa_browser_action", "qa_record_reproduction"} <= {
                t.name for t in tools.tools
            }
            resource = await client.read_resource("qa://workflow")
            assert resource.contents[0].mimeType == "text/plain"
            assert resource.contents[0].text
            result = await client.call_tool(
                "qa_create_job",
                {
                    "contract": {
                        "title": "MCP smoke",
                        "target_kind": "software",
                        "build_id": "v1",
                        "scope": "Smoke",
                        "criteria": [{"id": "boot", "description": "Starts"}],
                    }
                },
            )
            assert not result.isError
            # Both MCP text JSON and structured results are valid protocol responses.
            job = result.structuredContent or json.loads(result.content[0].text)
            assert job["contract"]["build_id"] == "v1"
            run = await client.call_tool(
                "qa_begin_run", {"job_id": job["id"], "kind": "discovery", "fixture_description": "Fresh VM"}
            )
            assert not run.isError
            bad = await client.call_tool("qa_complete_job", {"job_id": job["id"], "outcomes": []})
            assert bad.isError
            good = await client.call_tool(
                "qa_complete_job",
                {
                    "job_id": job["id"],
                    "outcomes": [{"criterion_id": "boot", "result": "not_tested", "reason": "Smoke only"}],
                },
            )
            assert not good.isError
            report = await client.call_tool("qa_export_report", {"job_id": job["id"]})
            paths = report.structuredContent or json.loads(report.content[0].text)
            assert Path(paths["markdown"]).is_file()
