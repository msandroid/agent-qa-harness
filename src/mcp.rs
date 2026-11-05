use crate::{
    TOOLS, VERSION, WORKFLOW,
    browser::BrowserPool,
    store::{Store, text},
};
use anyhow::{Context, Result, ensure};
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex, Weak},
};
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone)]
pub struct Harness {
    pub store: Store,
    pub browser: BrowserPool,
    tools: Arc<Vec<Tool>>,
    validators: Arc<HashMap<String, jsonschema::Validator>>,
    workers: Arc<Semaphore>,
    limit: usize,
    gates: Arc<StdMutex<HashMap<String, Weak<Mutex<()>>>>>,
}
impl Harness {
    pub fn new(store: Store, max_concurrency: usize, max_browsers: usize) -> Result<Self> {
        let raw: Vec<Value> = serde_json::from_str(TOOLS)?;
        let validators = raw
            .iter()
            .map(|v| {
                Ok((
                    text(v, "name")?.to_owned(),
                    jsonschema::validator_for(&v["inputSchema"])?,
                ))
            })
            .collect::<Result<HashMap<_, _>>>()?;
        let tools = raw
            .into_iter()
            .map(serde_json::from_value)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Self {
            browser: BrowserPool::new(store.clone(), max_browsers),
            store,
            tools: Arc::new(tools),
            validators: Arc::new(validators),
            workers: Arc::new(Semaphore::new(max_concurrency)),
            limit: max_concurrency,
            gates: Arc::default(),
        })
    }
    pub async fn dispatch(&self, name: &str, args: Value) -> Result<Value> {
        // Backpressure is explicit: no unbounded queue of jobs waiting for memory or browser slots.
        let _permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .context("Harness busy: concurrency limit reached; retry later")?;
        self.validators
            .get(name)
            .context("Unknown tool")?
            .validate(&args)
            .map_err(|e| anyhow::anyhow!("Invalid arguments: {e}"))?;
        ensure!(
            serde_json::to_vec(&args)?.len() <= 1024 * 1024,
            "Tool arguments exceed 1 MiB"
        );
        let gate = if let Some(job) = args["job_id"].as_str() {
            let mut gates = self.gates.lock().unwrap();
            gates.retain(|_, v| v.strong_count() > 0);
            let gate = gates
                .get(job)
                .and_then(Weak::upgrade)
                .unwrap_or_else(|| Arc::new(Mutex::new(())));
            gates.insert(job.to_owned(), Arc::downgrade(&gate));
            Some(gate)
        } else {
            None
        };
        let this = self.clone();
        let name = name.to_owned();
        // A disconnected MCP caller must not cancel an already-started side effect midway.
        // The owned task retains capacity and the per-job gate until its receipt is finished.
        tokio::spawn(async move {
            let permit = _permit;
            let guard = match gate {
                Some(g) => Some(g.lock_owned().await),
                None => None,
            };
            match name.as_str() {
                "qa_browser_start" => {
                    let _keep = (permit, guard);
                    this.browser
                        .start(
                            text(&args, "job_id")?,
                            text(&args, "run_id")?,
                            args["headless"].as_bool().unwrap_or(true),
                        )
                        .await
                }
                "qa_browser_action" => {
                    let _keep = (permit, guard);
                    this.browser
                        .action(
                            text(&args, "job_id")?,
                            text(&args, "run_id")?,
                            &args["action"],
                        )
                        .await
                }
                "qa_browser_close" => {
                    let _keep = (permit, guard);
                    this.browser.close(text(&args, "job_id")?).await
                }
                _ => {
                    tokio::task::spawn_blocking(move || {
                        let _keep = (permit, guard);
                        this.store.execute(&name, &args)
                    })
                    .await?
                }
            }
        })
        .await?
    }
    pub async fn shutdown(&self) {
        let _drained = self.workers.acquire_many(self.limit as u32).await;
        self.browser.shutdown().await;
    }
}
impl ServerHandler for Harness {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("agent-qa-harness", VERSION))
        .with_instructions(WORKFLOW)
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tools.as_ref().clone(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let result = self
            .dispatch(
                request.name.as_ref(),
                Value::Object(request.arguments.unwrap_or_default()),
            )
            .await;
        let result = match result {
            Ok(v) => {
                if serde_json::to_vec(&v).map_or(true, |s| s.len() > 2 * 1024 * 1024) {
                    CallToolResult::error(vec![ContentBlock::text(
                        "Response exceeds 2 MiB; request fewer events or export the streamed report",
                    )])
                } else {
                    CallToolResult::structured(v)
                }
            }
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
        };
        Ok(result.into())
    }
    async fn list_resources(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(serde_json::from_value(json!({"resources":[{"uri":"qa://workflow","name":"workflow","mimeType":"text/plain"}]})).expect("static resource schema"))
    }
    async fn read_resource(
        &self,
        r: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if r.uri != "qa://workflow" {
            return Err(ErrorData::invalid_params("Unknown resource", None));
        }
        let r: ReadResourceResult = serde_json::from_value(
            json!({"contents":[{"uri":"qa://workflow","mimeType":"text/plain","text":WORKFLOW}]}),
        )
        .expect("static resource schema");
        Ok(r.into())
    }
    async fn list_prompts(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(serde_json::from_value(json!({"prompts":[{"name":"qa_explore","description":"Explore and reproduce quality issues","arguments":[{"name":"target","required":true},{"name":"build_id","required":true}]}]})).expect("static prompt schema"))
    }
    async fn get_prompt(
        &self,
        r: GetPromptRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        if r.name != "qa_explore" {
            return Err(ErrorData::invalid_params("Unknown prompt", None));
        }
        let a = r.arguments.unwrap_or_default();
        let target = a.get("target").and_then(|v| v.as_str()).unwrap_or("target");
        let build = a
            .get("build_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let r:GetPromptResult=serde_json::from_value(json!({"messages":[{"role":"user","content":{"type":"text","text":format!("Explore {target}, build {build}. {WORKFLOW}")}}]})).expect("static prompt schema");
        Ok(r.into())
    }
}
