import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from agent_qa.browser import BrowserDriver, origin
from agent_qa.models import BrowserAction, Contract, FindingInput, Outcome
from agent_qa.store import HarnessError, Store

HTML = b"""<!doctype html><title>QA fixture</title><label>Name <input id="name"></label>
<button id="save" onclick="document.querySelector('#status').textContent='Saved: '+document.querySelector('#name').value">Save</button>
<p id="status">Not saved</p>"""


@pytest.fixture
def website():
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.end_headers()
            self.wfile.write(HTML)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{server.server_port}"
    server.shutdown()
    server.server_close()
    thread.join()


@pytest.mark.browser
async def test_browser_discovers_and_reproduces_real_save_defect(tmp_path, website):
    store = Store(tmp_path / "data")
    job = store.create(
        Contract(
            title="Browser acceptance",
            target_kind="web",
            build_id="buggy-fixture",
            scope="Save must survive reload",
            allowed_origins=[website],
            criteria=[{"id": "save", "description": "Name persists on reload"}],
            max_steps=30,
        ),
        str(tmp_path),
    )
    driver = BrowserDriver(store)
    try:
        run = store.begin_run(job["id"], "discovery", "Fresh fixture, no stored data")
        await driver.start(job["id"], run["id"])

        async def operate(run):
            for action in [
                BrowserAction(kind="navigate", url=website),
                BrowserAction(kind="fill", selector="#name", value="Ada"),
                BrowserAction(kind="click", selector="#save"),
            ]:
                result = await driver.action(job["id"], run["id"], action)
                assert result["state"] == "observed"
            assert "Saved: Ada" in result["observation"]["visible_text"]
            result = await driver.action(job["id"], run["id"], BrowserAction(kind="navigate", url=website))
            assert "Not saved" in result["observation"]["visible_text"]
            return result["observation"]["observation_artifact_id"]

        evidence = await operate(run)
        f = store.finding(
            job["id"],
            run["id"],
            FindingInput(
                title="Save falsely reports success",
                expected="Ada remains after reload",
                actual="Not saved after reload",
                steps=["Fill Ada", "Save", "Reload"],
                severity="high",
                criterion_id="save",
                artifact_ids=[evidence],
            ),
        )
        with pytest.raises(HarnessError, match="outside"):
            await driver.action(
                job["id"], run["id"], BrowserAction(kind="navigate", url="https://example.com")
            )
        await driver.close(job["id"])
        rerun = store.begin_run(job["id"], "reproduction", "New isolated browser context, fresh fixture")
        await driver.start(job["id"], rerun["id"])
        new_evidence = await operate(rerun)
        confirmed = store.reproduce(
            job["id"], f["id"], rerun["id"], True, [new_evidence], "Repeated save loss"
        )
        assert confirmed["status"] == "confirmed"
        unknown = await driver.action(
            job["id"], rerun["id"], BrowserAction(kind="click", selector="#missing", timeout_ms=100)
        )
        assert unknown["state"] == "unknown"
        assert unknown["observation"]["error"]
        store.complete(
            job["id"],
            [
                Outcome(
                    criterion_id="save", result="fail", reason="Loss reproduced", artifact_ids=[new_evidence]
                )
            ],
        )
        assert store.export(job["id"])["job_status"] == "completed"
    finally:
        await driver.shutdown()


def test_origin_credentials_and_non_http_rejected():
    for url in ["file:///etc/passwd", "javascript:alert(1)", "http://user:pass@example.com"]:
        with pytest.raises(HarnessError):
            origin(url)
