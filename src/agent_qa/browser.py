"""Optional first-party browser driver. No shell or arbitrary JavaScript tool."""

import asyncio
import json
from urllib.parse import urlsplit

from .models import BrowserAction
from .store import HarnessError, Store


def origin(url: str) -> str:
    p = urlsplit(url)
    if p.scheme not in {"http", "https"} or not p.hostname or p.username or p.password:
        raise HarnessError("Only credential-free HTTP(S) URLs are allowed")
    port = p.port
    host = p.hostname.lower()
    host = f"[{host}]" if ":" in host else host
    suffix = f":{port}" if port and port != {"http": 80, "https": 443}[p.scheme] else ""
    return f"{p.scheme}://{host}{suffix}"


class BrowserDriver:
    def __init__(self, store: Store):
        self.store = store
        self.sessions = {}
        self.locks = {}
        self.playwright = None

    def lock(self, job_id):
        return self.locks.setdefault(job_id, asyncio.Lock())

    async def start(self, job_id, run_id, headless=True):
        async with self.lock(job_id):
            if job_id in self.sessions:
                raise HarnessError("Close the existing browser before starting a fresh run")
            j = self.store.get(job_id)
            allowed = set(j["contract"]["allowed_origins"])
            if not allowed:
                raise HarnessError("Browser requires exact allowed_origins in the job contract")
            receipt = self.store.reserve_step(job_id, run_id, "start isolated Chromium", "browser_collected")
            browser = None
            try:
                from playwright.async_api import async_playwright

                if self.playwright is None:
                    self.playwright = await async_playwright().start()
                browser = await self.playwright.chromium.launch(headless=headless)
                context = await browser.new_context(accept_downloads=False, service_workers="block")
                logs = []

                async def guard(route):
                    try:
                        permitted = origin(route.request.url) in allowed
                    except (ValueError, HarnessError):
                        permitted = False
                    if permitted:
                        await route.continue_()
                    else:
                        logs.append({"kind": "blocked_request", "url": route.request.url})
                        del logs[:-200]
                        await route.abort()

                await context.route("**/*", guard)
                # WebSockets otherwise bypass HTTP routing.
                await context.route_web_socket("**/*", lambda socket: socket.close())
                page = await context.new_page()

                def log(kind, message):
                    logs.append({"kind": kind, "message": str(message)[:4000]})
                    del logs[:-200]

                page.on("console", lambda m: log(f"console:{m.type}", m.text))
                page.on("pageerror", lambda e: log("pageerror", e))

                async def close_popup(popup):
                    await popup.close()

                context.on("page", close_popup)
                self.sessions[job_id] = {
                    "browser": browser,
                    "context": context,
                    "page": page,
                    "run_id": run_id,
                    "logs": logs,
                    "allowed": allowed,
                }
                self.store.finish_step(
                    job_id, receipt, "Fresh isolated browser context; target data not reset", "observed"
                )
                return {
                    "run_id": run_id,
                    "fresh_browser_context": True,
                    "target_reset_verified": False,
                    "websockets": "blocked",
                    "popups": "closed",
                }
            except Exception as e:
                if browser:
                    await browser.close()
                self.store.finish_step(job_id, receipt, str(e)[:4000], "failed")
                raise

    async def action(self, job_id, run_id, action: BrowserAction):
        async with self.lock(job_id):
            s = self.sessions.get(job_id)
            if not s or s["run_id"] != run_id:
                raise HarnessError("No live browser for this run; start a fresh context after a restart")
            if action.kind == "navigate" and origin(action.url) not in s["allowed"]:
                raise HarnessError("Navigation origin is outside the job contract")
            recorded = action.model_dump()
            if action.kind == "fill":
                recorded["value"] = "[redacted]"
            receipt = self.store.reserve_step(job_id, run_id, recorded, "browser_collected")
            page = s["page"]
            evidence = []
            error = None

            async def capture(label):
                try:
                    png = await page.screenshot(timeout=3000)
                    return self.store.attach_bytes(
                        job_id, run_id, "screen.png", png, label, "browser_collected"
                    )
                except Exception as e:
                    return {"capture_error": str(e)[:500]}

            evidence.append(await capture("before action"))
            try:
                if action.kind == "navigate":
                    await page.goto(action.url, wait_until="domcontentloaded", timeout=action.timeout_ms)
                elif action.kind != "observe":
                    locator = page.locator(action.selector)
                    if action.kind == "click":
                        await locator.click(timeout=action.timeout_ms)
                    elif action.kind == "fill":
                        await locator.fill(action.value, timeout=action.timeout_ms)
                    elif action.kind == "press":
                        await locator.press(action.value, timeout=action.timeout_ms)
                    elif action.kind == "wait_for":
                        await locator.wait_for(state="visible", timeout=action.timeout_ms)
            except Exception as e:
                # A timed-out action may have committed. It is not automatically retried.
                error = str(e)[:4000]
            evidence.append(await capture("after action"))
            observation = {"url": page.url, "error": error, "logs": list(s["logs"]), "artifacts": evidence}
            try:
                observation["visible_text"] = (await page.locator("body").inner_text(timeout=2000))[:16000]
            except Exception:
                observation["visible_text"] = "[unavailable]"
            trace = self.store.attach_bytes(
                job_id,
                run_id,
                "observation.json",
                json.dumps(observation, ensure_ascii=False).encode(),
                "browser observation",
                "browser_collected",
            )
            observation["observation_artifact_id"] = trace["id"]
            state = "unknown" if error else "observed"
            self.store.finish_step(job_id, receipt, observation, state)
            return receipt | {"state": state, "observation": observation}

    async def close(self, job_id):
        async with self.lock(job_id):
            s = self.sessions.pop(job_id, None)
            if s:
                await s["browser"].close()
            return {"closed": bool(s)}

    async def shutdown(self):
        for job_id in list(self.sessions):
            await self.close(job_id)
        if self.playwright:
            await self.playwright.stop()
