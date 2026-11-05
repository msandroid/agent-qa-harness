//! Bounded native Chrome DevTools transport. No Python, Node, arbitrary-JS or shell tool.
use crate::store::{Store, text};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot},
    time::{sleep, timeout},
};
use tokio_tungstenite::tungstenite::Message;

pub fn origin(value: &str) -> Result<String> {
    let u = url::Url::parse(value)?;
    ensure!(
        ["http", "https"].contains(&u.scheme())
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none(),
        "Only credential-free HTTP(S) URLs are allowed"
    );
    Ok(u.origin().ascii_serialization())
}

#[derive(Clone)]
struct Policy {
    allowed: HashSet<String>,
    logs: Arc<Mutex<VecDeque<Value>>>,
    loads: Arc<AtomicU64>,
}
#[derive(Clone)]
struct Cdp {
    tx: mpsc::Sender<Value>,
    pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
    next: Arc<AtomicU64>,
    policies: Arc<Mutex<HashMap<String, Policy>>>,
}
struct PendingGuard {
    id: u64,
    pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.pending.lock().unwrap().remove(&self.id);
    }
}
impl Cdp {
    async fn connect(address: &str) -> Result<Self> {
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(8 * 1024 * 1024))
            .max_frame_size(Some(8 * 1024 * 1024));
        let (socket, _) =
            tokio_tungstenite::connect_async_with_config(address, Some(config), false).await?;
        let (mut sink, mut stream) = socket.split();
        let (tx, mut rx) = mpsc::channel::<Value>(64);
        let c = Self {
            tx,
            pending: Arc::default(),
            next: Arc::new(AtomicU64::new(1)),
            policies: Arc::default(),
        };
        let actor = c.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    command=rx.recv()=>{let Some(command)=command else{break};if sink.send(Message::Text(command.to_string().into())).await.is_err(){break}},
                    response=stream.next()=>{
                        let raw=match response {Some(Ok(Message::Text(raw)))=>raw,Some(Ok(Message::Ping(data)))=>{let _=sink.send(Message::Pong(data)).await;continue;},Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break,_=>continue};
                        let Ok(v)=serde_json::from_str::<Value>(&raw) else{continue};
                        if let Some(id)=v["id"].as_u64(){if let Some(waiter)=actor.pending.lock().unwrap().remove(&id){let _=waiter.send(if v.get("error").is_some(){Err(anyhow::anyhow!("CDP: {}",v["error"]))}else{Ok(v["result"].clone())});}continue;}
                        let session=v["sessionId"].as_str().unwrap_or("");let policy=actor.policies.lock().await.get(session).cloned();
                        if let Some(p)=policy {let method=v["method"].as_str().unwrap_or("");
                            if method=="Fetch.requestPaused" {
                                let url=v["params"]["request"]["url"].as_str().unwrap_or("");
                                let allowed=origin(url).is_ok_and(|o|p.allowed.contains(&o));
                                let method=if allowed{"Fetch.continueRequest"}else{"Fetch.failRequest"};
                                let mut params=json!({"requestId":v["params"]["requestId"]});if !allowed{params["errorReason"]=json!("BlockedByClient");}
                                let command=json!({"id":actor.next.fetch_add(1,Ordering::Relaxed),"sessionId":session,"method":method,"params":params});
                                if sink.send(Message::Text(command.to_string().into())).await.is_err(){break}
                                if !allowed{push_log(&p.logs,json!({"kind":"blocked_request","url":short(url,4000)})).await;}
                            } else if method=="Page.domContentEventFired" {p.loads.fetch_add(1,Ordering::Relaxed);}
                            else if method=="Runtime.consoleAPICalled" || method=="Runtime.exceptionThrown" {push_log(&p.logs,json!({"kind":method,"message":short(&v["params"].to_string(),4000)})).await;}
                            else if method=="Page.windowOpen" { /* Creation is disabled by the browser launch flags; no permission granted. */ }
                        }
                    }
                }
            }
            for (_, waiter) in actor.pending.lock().unwrap().drain() {
                let _ = waiter.send(Err(anyhow::anyhow!("Browser disconnected")));
            }
        });
        Ok(c)
    }
    async fn cmd(&self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let _guard = PendingGuard {
            id,
            pending: self.pending.clone(),
        };
        let mut v = json!({"id":id,"method":method,"params":params});
        if let Some(s) = session {
            v["sessionId"] = json!(s);
        }
        let result = timeout(Duration::from_secs(35), async {
            self.tx
                .send(v)
                .await
                .context("Browser command queue closed")?;
            rx.await.context("Browser response cancelled")?
        })
        .await;
        result.context("Browser command timeout; result uncertain")?
    }
    async fn eval(&self, session: &str, expression: String) -> Result<Value> {
        let r = self
            .cmd(
                Some(session),
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
            )
            .await?;
        if r.get("exceptionDetails").is_some() {
            bail!("Browser evaluation: {}", r["exceptionDetails"])
        }
        Ok(r["result"]["value"].clone())
    }
}
fn short(s: &str, chars: usize) -> String {
    s.chars().take(chars).collect()
}
async fn push_log(logs: &Mutex<VecDeque<Value>>, v: Value) {
    let mut logs = logs.lock().await;
    if logs.len() >= 200 {
        logs.pop_front();
    }
    logs.push_back(v);
}

struct Chrome {
    cdp: Cdp,
    process: Mutex<Child>,
    _profile: tempfile::TempDir,
    headless: bool,
}
impl Chrome {
    async fn launch(headless: bool) -> Result<Self> {
        let executable = chrome_path()
            .context("Chromium not found; set QA_CHROME to a Chrome/Chromium executable")?;
        let profile = tempfile::tempdir()?;
        let mut cmd = Command::new(executable);
        cmd.arg(format!("--user-data-dir={}", profile.path().display()))
            .args([
                "--remote-debugging-port=0",
                "--remote-debugging-address=127.0.0.1",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-extensions",
                "--block-new-web-contents",
                "--window-size=1280,720",
                "about:blank",
            ]);
        if headless {
            cmd.arg("--headless=new");
        }
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);
        let mut process = cmd.spawn()?;
        let address = timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(s) =
                    tokio::fs::read_to_string(profile.path().join("DevToolsActivePort")).await
                {
                    let mut lines = s.lines();
                    if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                        return Ok::<_, anyhow::Error>(format!("ws://127.0.0.1:{port}{path}"));
                    }
                }
                ensure!(
                    process.try_wait()?.is_none(),
                    "Chromium exited during startup"
                );
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .context("Chromium startup timeout")??;
        Ok(Self {
            cdp: Cdp::connect(&address).await?,
            process: Mutex::new(process),
            _profile: profile,
            headless,
        })
    }
    async fn close(&self) {
        let _ = timeout(
            Duration::from_secs(3),
            self.cdp.cmd(None, "Browser.close", json!({})),
        )
        .await;
        let _ = self.process.lock().await.kill().await;
    }
}
pub fn chrome_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("QA_CHROME") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut paths = vec![
        PathBuf::from("/usr/bin/chromium"),
        PathBuf::from("/usr/bin/chromium-browser"),
        PathBuf::from("/usr/bin/google-chrome"),
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
    ];
    for root in [
        std::env::var_os("PROGRAMFILES"),
        std::env::var_os("LOCALAPPDATA"),
    ]
    .into_iter()
    .flatten()
    {
        paths.push(PathBuf::from(root).join("Google/Chrome/Application/chrome.exe"));
    }
    if let Some(root) = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("ms-playwright"))
        && let Ok(entries) = std::fs::read_dir(root)
    {
        let mut entries = entries.flatten().map(|x| x.path()).collect::<Vec<_>>();
        entries.sort();
        entries.reverse();
        for p in entries {
            for suffix in [
                "chrome-win64/chrome.exe",
                "chrome-win/chrome.exe",
                "chrome-linux64/chrome",
                "chrome-linux/chrome",
            ] {
                paths.push(p.join(suffix));
            }
        }
    }
    paths.into_iter().find(|p| p.is_file())
}
struct Session {
    chrome: Arc<Chrome>,
    context: String,
    session: String,
    run: String,
    logs: Arc<Mutex<VecDeque<Value>>>,
    loads: Arc<AtomicU64>,
    allowed: HashSet<String>,
    _permit: OwnedSemaphorePermit,
}
type JobSlot = Arc<Mutex<Option<Session>>>;
#[derive(Clone)]
pub struct BrowserPool {
    store: Store,
    chrome: Arc<Mutex<Option<Arc<Chrome>>>>,
    jobs: Arc<Mutex<HashMap<String, JobSlot>>>,
    capacity: Arc<Semaphore>,
}
impl BrowserPool {
    pub fn new(store: Store, max_sessions: usize) -> Self {
        Self {
            store,
            chrome: Arc::default(),
            jobs: Arc::default(),
            capacity: Arc::new(Semaphore::new(max_sessions)),
        }
    }
    async fn slot(&self, job: &str) -> JobSlot {
        let mut jobs = self.jobs.lock().await;
        jobs.retain(|_, slot| {
            Arc::strong_count(slot) > 1 || slot.try_lock().map_or(true, |s| s.is_some())
        });
        jobs.entry(job.to_owned()).or_default().clone()
    }
    pub async fn start(&self, job: &str, run: &str, headless: bool) -> Result<Value> {
        let slot = self.slot(job).await;
        let mut slot = slot.lock().await;
        ensure!(
            slot.is_none(),
            "Close the existing browser before starting a fresh run"
        );
        let permit =
            self.capacity.clone().try_acquire_owned().context(
                "Browser session limit reached; close another job's owned session first",
            )?;
        let j = self.store.basic(job)?;
        let allowed = j["contract"]["allowed_origins"]
            .as_array()
            .context("Missing origins")?
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_owned())
            .collect::<HashSet<_>>();
        ensure!(
            !allowed.is_empty(),
            "Browser requires exact allowed_origins"
        );
        let receipt = self.store.reserve(
            job,
            run,
            json!("start isolated Chromium context"),
            "browser_collected",
        )?;
        let result=async {
            let chrome={let mut shared=self.chrome.lock().await;if shared.is_none(){*shared=Some(Arc::new(Chrome::launch(headless).await?));}
                let c=shared.as_ref().unwrap().clone();ensure!(c.headless==headless,"Shared Chromium already uses a different headless mode");c};
            let c=&chrome.cdp;let context=text(&c.cmd(None,"Target.createBrowserContext",json!({"disposeOnDetach":true})).await?,"browserContextId")?.to_owned();
            let mut registered_session = None;
            let setup=async {
                c.cmd(None,"Browser.setDownloadBehavior",json!({"behavior":"deny","browserContextId":context})).await?;
                let target=text(&c.cmd(None,"Target.createTarget",json!({"url":"about:blank","browserContextId":context})).await?,"targetId")?.to_owned();
                let session=text(&c.cmd(None,"Target.attachToTarget",json!({"targetId":target,"flatten":true})).await?,"sessionId")?.to_owned();
                let logs=Arc::new(Mutex::new(VecDeque::new()));let loads=Arc::new(AtomicU64::new(0));
                c.policies.lock().await.insert(session.clone(),Policy{allowed:allowed.clone(),logs:logs.clone(),loads:loads.clone()});
                registered_session = Some(session.clone());
                c.cmd(Some(&session),"Page.enable",json!({})).await?;c.cmd(Some(&session),"Runtime.enable",json!({})).await?;
                c.cmd(Some(&session),"Network.enable",json!({"maxTotalBufferSize":65536,"maxResourceBufferSize":16384})).await?;
                c.cmd(Some(&session),"Network.setBypassServiceWorker",json!({"bypass":true})).await?;
                c.cmd(Some(&session),"Network.setBlockedURLs",json!({"urls":["ws://*","wss://*"]})).await?;
                c.cmd(Some(&session),"Emulation.setDeviceMetricsOverride",json!({"width":1280,"height":720,"deviceScaleFactor":1,"mobile":false})).await?;
                c.cmd(Some(&session),"Page.addScriptToEvaluateOnNewDocument",json!({"source":"Object.defineProperty(window,'WebSocket',{value:class{constructor(){throw Error('WebSockets disabled by QA driver')}}});Object.defineProperty(navigator,'serviceWorker',{get(){throw Error('Service workers disabled by QA driver')}});"})).await?;
                c.cmd(Some(&session),"Fetch.enable",json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]})).await?;
                Ok::<_,anyhow::Error>((session,logs,loads))
            }.await;
            match setup {Ok((session,logs,loads))=>Ok(Session{chrome:chrome.clone(),context,session,run:run.to_owned(),logs,loads,allowed,_permit:permit}),
                Err(e)=>{if let Some(session)=registered_session {c.policies.lock().await.remove(&session);}let _=c.cmd(None,"Target.disposeBrowserContext",json!({"browserContextId":context})).await;Err(e)}}
        }.await;
        match result {
            Ok(s) => {
                *slot = Some(s);
                self.store.finish(
                    job,
                    &receipt,
                    &json!("Fresh isolated browser context; target data not reset"),
                    "observed",
                )?;
                Ok(
                    json!({"run_id":run,"fresh_browser_context":true,"target_reset_verified":false,"websockets":"blocked","popups":"blocked","engine":"native-cdp-shared-chromium"}),
                )
            }
            Err(e) => {
                self.store
                    .finish(job, &receipt, &json!(e.to_string()), "failed")?;
                Err(e)
            }
        }
    }
    pub async fn action(&self, job: &str, run: &str, a: &Value) -> Result<Value> {
        let slot = self.slot(job).await;
        let slot = slot.lock().await;
        let s = slot
            .as_ref()
            .context("No live browser; start a fresh context after restart")?;
        ensure!(s.run == run, "No live browser for this run");
        let kind = text(a, "kind")?;
        if kind == "navigate" {
            ensure!(
                s.allowed.contains(&origin(text(a, "url")?)?),
                "Navigation origin is outside the job contract"
            );
        }
        let mut recorded = a.clone();
        if kind == "fill" {
            recorded["value"] = json!("[redacted]");
        }
        let receipt = self
            .store
            .reserve(job, run, recorded, "browser_collected")?;
        let mut artifacts = Vec::new();
        artifacts.push(self.capture(s, job, run, "before action").await);
        let timeout_ms = a["timeout_ms"].as_u64().unwrap_or(5000).clamp(100, 30000);
        let operation = timeout(Duration::from_millis(timeout_ms), Self::operate(s, a)).await;
        let error = match operation {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(short(&e.to_string(), 4000)),
            Err(_) => Some("Action timeout; result uncertain. Inspect before retrying.".to_owned()),
        };
        artifacts.push(self.capture(s, job, run, "after action").await);
        let mut observation=s.chrome.cdp.eval(&s.session,"({url:location.href,visible_text:(document.body?.innerText||'').slice(0,16000),controls:Array.from(document.querySelectorAll('input,textarea,select,button')).filter(e=>e.getClientRects().length).slice(0,80).map(e=>({tag:e.tagName,id:e.id,label:e.getAttribute('aria-label')||'',type:e.type||'',value:e.type==='password'?'[redacted]':String(e.value||'').slice(0,1000),text:(e.innerText||'').slice(0,200)}))})".to_owned()).await.unwrap_or_else(|e|json!({"observation_error":e.to_string()}));
        observation["error"] = json!(error);
        observation["logs"] = json!(s.logs.lock().await.iter().cloned().collect::<Vec<_>>());
        observation["artifacts"] = json!(artifacts);
        let evidence = self.store.attach_bytes(
            job,
            run,
            "observation.json",
            &serde_json::to_vec(&observation)?,
            "browser observation",
        )?;
        observation["observation_artifact_id"] = evidence["id"].clone();
        self.store.finish(
            job,
            &receipt,
            &observation,
            if error.is_some() {
                "unknown"
            } else {
                "observed"
            },
        )
    }
    async fn capture(&self, s: &Session, job: &str, run: &str, label: &str) -> Value {
        let result = async {
            let result = timeout(
                Duration::from_secs(4),
                s.chrome.cdp.cmd(
                    Some(&s.session),
                    "Page.captureScreenshot",
                    json!({"format":"png","captureBeyondViewport":false}),
                ),
            )
            .await??;
            let bytes = STANDARD.decode(text(&result, "data")?)?;
            self.store
                .attach_bytes(job, run, "screen.png", &bytes, label)
        }
        .await;
        result.unwrap_or_else(|e: anyhow::Error| json!({"capture_error":short(&e.to_string(),500)}))
    }
    async fn operate(s: &Session, a: &Value) -> Result<()> {
        let c = &s.chrome.cdp;
        let kind = text(a, "kind")?;
        if kind == "observe" {
            return Ok(());
        }
        if kind == "navigate" {
            let before = s.loads.load(Ordering::Relaxed);
            let result = c
                .cmd(
                    Some(&s.session),
                    "Page.navigate",
                    json!({"url":text(a,"url")?}),
                )
                .await?;
            if let Some(error) = result.get("errorText") {
                bail!("Navigation failed: {error}")
            }
            while s.loads.load(Ordering::Relaxed) == before {
                sleep(Duration::from_millis(20)).await;
            }
            return Ok(());
        }
        let selector = text(a, "selector")?;
        let expression = format!(
            "(()=>{{const s={};{};const e=find(s);if(!e||!e.getClientRects().length)return null;e.scrollIntoView({{block:'center'}});const r=e.getBoundingClientRect();return {{x:r.x+r.width/2,y:r.y+r.height/2}}}})()",
            serde_json::to_string(selector)?,
            FIND
        );
        let position = loop {
            let p = c.eval(&s.session, expression.clone()).await?;
            if !p.is_null() {
                break p;
            }
            sleep(Duration::from_millis(25)).await;
        };
        if kind == "wait_for" {
            return Ok(());
        }
        if kind == "click" {
            for (t, b, buttons) in [
                ("mouseMoved", "none", 0),
                ("mousePressed", "left", 1),
                ("mouseReleased", "left", 0),
            ] {
                c.cmd(Some(&s.session),"Input.dispatchMouseEvent",json!({"type":t,"x":position["x"],"y":position["y"],"button":b,"buttons":buttons,"clickCount":1})).await?;
            }
        } else if kind == "fill" {
            c.eval(&s.session,format!("(()=>{{{};const e=find({});if(!e||!['INPUT','TEXTAREA'].includes(e.tagName))throw Error('fill requires input/textarea');e.focus();e.select();return true}})()",FIND,serde_json::to_string(selector)?)).await?;
            c.cmd(
                Some(&s.session),
                "Input.insertText",
                json!({"text":text(a,"value")?}),
            )
            .await?;
        } else if kind == "press" {
            c.eval(
                &s.session,
                format!(
                    "(()=>{{{};find({}).focus();return true}})()",
                    FIND,
                    serde_json::to_string(selector)?
                ),
            )
            .await?;
            let value = text(a, "value")?;
            let mut parts = value.split('+').collect::<Vec<_>>();
            let key = parts.pop().context("Missing key")?;
            let mut modifiers = 0;
            for m in parts {
                modifiers |= match m.to_lowercase().as_str() {
                    "alt" => 1,
                    "control" | "ctrl" => 2,
                    "meta" | "command" => 4,
                    "shift" => 8,
                    _ => bail!("Unsupported key modifier"),
                };
            }
            let (key, code, vk, typed) = match key {
                "Enter" => ("Enter".to_owned(), "Enter".to_owned(), 13, "\r".to_owned()),
                "Tab" => ("Tab".to_owned(), "Tab".to_owned(), 9, String::new()),
                "Escape" => ("Escape".to_owned(), "Escape".to_owned(), 27, String::new()),
                "Backspace" => (
                    "Backspace".to_owned(),
                    "Backspace".to_owned(),
                    8,
                    String::new(),
                ),
                "Delete" => ("Delete".to_owned(), "Delete".to_owned(), 46, String::new()),
                k if k.len() == 1 && k.is_ascii() => (
                    k.to_owned(),
                    format!("Key{}", k.to_uppercase()),
                    k.to_ascii_uppercase().as_bytes()[0] as i32,
                    if modifiers == 0 {
                        k.to_owned()
                    } else {
                        String::new()
                    },
                ),
                _ => bail!(
                    "Unsupported key; use Enter/Tab/Escape/Backspace/Delete or a single ASCII character"
                ),
            };
            c.cmd(Some(&s.session),"Input.dispatchKeyEvent",json!({"type":"keyDown","key":key,"code":code,"windowsVirtualKeyCode":vk,"modifiers":modifiers,"text":typed})).await?;
            c.cmd(Some(&s.session),"Input.dispatchKeyEvent",json!({"type":"keyUp","key":key,"code":code,"windowsVirtualKeyCode":vk,"modifiers":modifiers})).await?;
        } else {
            bail!("Unknown browser action")
        }
        c.eval(
            &s.session,
            "new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(true))))"
                .to_owned(),
        )
        .await?;
        Ok(())
    }
    pub async fn close(&self, job: &str) -> Result<Value> {
        let slot = self.slot(job).await;
        let mut slot = slot.lock().await;
        if let Some(s) = slot.as_ref() {
            s.chrome
                .cdp
                .cmd(
                    None,
                    "Target.disposeBrowserContext",
                    json!({"browserContextId":s.context}),
                )
                .await?;
            s.chrome.cdp.policies.lock().await.remove(&s.session);
            slot.take();
            return Ok(json!({"closed":true}));
        }
        Ok(json!({"closed":false}))
    }
    pub async fn shutdown(&self) {
        let jobs = self.jobs.lock().await.keys().cloned().collect::<Vec<_>>();
        for job in jobs {
            let _ = self.close(&job).await;
        }
        if let Some(chrome) = self.chrome.lock().await.take() {
            chrome.close().await;
        }
    }
}
const FIND: &str = r#"function find(s){if(s.startsWith('text=')){let t=s.slice(5);return [...document.querySelectorAll('button,a,label')].find(e=>e.textContent.trim().includes(t))}const m=s.match(/^(.*):has-text\((['"])(.*?)\2\)$/);if(m)return [...document.querySelectorAll(m[1]||'*')].find(e=>e.textContent.includes(m[3]));return document.querySelector(s)}"#;
