use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_ARTIFACT: u64 = 25 * 1024 * 1024;
pub const PAGE_SIZE: usize = 500;

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
pub fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .with_context(|| format!("Missing string: {k}"))
}
fn strings(v: &Value) -> Result<Vec<String>> {
    v.as_array()
        .context("Expected array")?
        .iter()
        .map(|x| Ok(x.as_str().context("Expected string")?.to_owned()))
        .collect()
}

#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
    pub workspace: PathBuf,
}

impl Store {
    pub fn new(root: &Path, workspace: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let s = Self {
            root: root.canonicalize()?,
            workspace: workspace.canonicalize()?,
        };
        ensure!(s.workspace.is_dir(), "workspace must be a directory");
        s.connect()?.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY,job TEXT NOT NULL,data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS artifacts(id TEXT PRIMARY KEY,job TEXT NOT NULL,run TEXT NOT NULL,data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS findings(id TEXT PRIMARY KEY,job TEXT NOT NULL,data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,job TEXT NOT NULL,at REAL NOT NULL,kind TEXT NOT NULL,data TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS qa_runs_job ON runs(job);
            CREATE INDEX IF NOT EXISTS qa_artifacts_job ON artifacts(job);
            CREATE INDEX IF NOT EXISTS qa_findings_job ON findings(job);
            CREATE INDEX IF NOT EXISTS qa_events_job_seq ON events(job,seq);")?;
        Ok(s)
    }
    pub fn connect(&self) -> Result<Connection> {
        let c = Connection::open(self.root.join("ledger.sqlite3"))?;
        c.busy_timeout(Duration::from_secs(30))?;
        // Each short-lived connection has a bounded SQLite cache, not an unbounded pool.
        c.execute_batch("PRAGMA cache_size=-512; PRAGMA temp_store=FILE;")?;
        Ok(c)
    }
    pub fn row(c: &Connection, table: &str, ident: &str) -> Result<Value> {
        let s: Option<String> = c
            .query_row(
                &format!("SELECT data FROM {table} WHERE id=?1"),
                [ident],
                |r| r.get(0),
            )
            .optional()?;
        serde_json::from_str(&s.with_context(|| format!("Unknown {table}: {ident}"))?)
            .map_err(Into::into)
    }
    fn put(c: &Connection, table: &str, v: &Value) -> Result<()> {
        c.execute(
            &format!("UPDATE {table} SET data=?1 WHERE id=?2"),
            params![v.to_string(), text(v, "id")?],
        )?;
        Ok(())
    }
    fn event(c: &Connection, job: &str, kind: &str, v: &Value) -> Result<()> {
        c.execute(
            "INSERT INTO events(job,at,kind,data) VALUES(?1,?2,?3,?4)",
            params![job, now(), kind, v.to_string()],
        )?;
        Ok(())
    }
    fn active(j: &Value) -> Result<()> {
        ensure!(j["status"] == "active", "Job is not active");
        Ok(())
    }
    fn writable(j: &Value) -> Result<()> {
        ensure!(j["status"] != "completed", "Completed jobs are immutable");
        Ok(())
    }
    fn budget(j: &Value) -> Result<()> {
        Self::active(j)?;
        let c = &j["contract"];
        ensure!(
            now() - j["created_at"].as_f64().context("Invalid creation time")?
                < c["max_seconds"].as_f64().context("Invalid deadline")?,
            "Wall-clock budget exhausted (includes paused time)"
        );
        ensure!(
            j["steps_used"].as_u64().context("Invalid step count")?
                < c["max_steps"].as_u64().context("Invalid budget")?,
            "Step budget exhausted"
        );
        ensure!(
            j["reported_cost_usd"].as_f64().unwrap_or(0.)
                <= c["max_reported_cost_usd"].as_f64().unwrap_or(10.),
            "Reported cost budget exhausted"
        );
        Ok(())
    }
    pub fn basic(&self, job: &str) -> Result<Value> {
        Self::row(&self.connect()?, "jobs", job)
    }
    pub fn run(&self, job: &str, run: &str) -> Result<Value> {
        let r = Self::row(&self.connect()?, "runs", run)?;
        ensure!(r["job_id"] == job, "Run belongs to another job");
        Ok(r)
    }
    pub fn snapshot(&self, job: &str) -> Result<Value> {
        let c = self.connect()?;
        let mut j = Self::row(&c, "jobs", job)?;
        let mut truncated = Vec::new();
        for t in ["runs", "artifacts", "findings"] {
            let mut st = c.prepare(&format!(
                "SELECT data FROM {t} WHERE job=?1 ORDER BY rowid LIMIT ?2"
            ))?;
            let mut values = st
                .query_map(params![job, PAGE_SIZE + 1], |r| r.get::<_, String>(0))?
                .map(|s| Ok(serde_json::from_str::<Value>(&s?)?))
                .collect::<Result<Vec<_>>>()?;
            if values.len() > PAGE_SIZE {
                values.pop();
                truncated.push(t);
            }
            j[t] = json!(values);
        }
        j["truncated_fields"] = json!(truncated);
        j["record_limit"] = json!(PAGE_SIZE);
        Ok(j)
    }
    pub fn events(&self, job: &str, after: i64, limit: usize) -> Result<Value> {
        let c = self.connect()?;
        Self::row(&c, "jobs", job)?;
        let mut st = c.prepare(
            "SELECT seq,at,kind,data FROM events WHERE job=?1 AND seq>?2 ORDER BY seq LIMIT ?3",
        )?;
        let v=st.query_map(params![job,after,limit.clamp(1,100)], |r|Ok((r.get::<_,i64>(0)?,r.get::<_,f64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))?
            .map(|r| {let (seq,at,kind,data)=r?;Ok(json!({"seq":seq,"at":at,"kind":kind,"data":serde_json::from_str::<Value>(&data)?}))}).collect::<Result<Vec<_>>>()?;
        Ok(json!(v))
    }
    pub fn reserve(&self, job: &str, run: &str, action: Value, source: &str) -> Result<Value> {
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut j = Self::row(&tx, "jobs", job)?;
        Self::budget(&j)?;
        let r = Self::row(&tx, "runs", run)?;
        ensure!(r["job_id"] == job, "Run belongs to another job");
        j["steps_used"] = json!(j["steps_used"].as_u64().unwrap_or(0) + 1);
        Self::put(&tx, "jobs", &j)?;
        let r = json!({"action_id":id(),"run_id":run,"number":j["steps_used"],"action":action,"source":source,"state":"started"});
        Self::event(&tx, job, "step_started", &r)?;
        tx.commit()?;
        Ok(r)
    }
    pub fn finish(
        &self,
        job: &str,
        receipt: &Value,
        observation: &Value,
        state: &str,
    ) -> Result<Value> {
        ensure!(
            ["observed", "unknown", "failed"].contains(&state),
            "Invalid step state"
        );
        let mut result = receipt.clone();
        result["observation"] = observation.clone();
        result["state"] = json!(state);
        Self::event(&self.connect()?, job, "step_finished", &result)?;
        Ok(result)
    }
    pub fn hash(path: &Path) -> Result<(String, u64)> {
        let mut f = File::open(path)?;
        let mut h = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut size = 0;
        loop {
            let n = f.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            h.update(&buffer[..n]);
            size += n as u64;
        }
        Ok((format!("{:x}", h.finalize()), size))
    }
    fn verify(&self, c: &Connection, job: &str, ids: &[String], run: Option<&str>) -> Result<()> {
        for ident in ids.iter().collect::<HashSet<_>>() {
            let a = Self::row(c, "artifacts", ident)?;
            ensure!(
                a["job_id"] == job && run.is_none_or(|r| a["run_id"] == r),
                "Evidence belongs to another job or run"
            );
            let path = self.root.join(text(&a, "stored_path")?).canonicalize()?;
            ensure!(
                path.starts_with(&self.root),
                "Stored artifact path escapes data root"
            );
            let (hash, size) = Self::hash(&path)?;
            ensure!(
                a["sha256"] == hash && a["size"] == size,
                "Artifact missing or modified"
            );
        }
        Ok(())
    }
    fn artifact_path(&self, job: &str, filename: &str) -> Result<(String, PathBuf)> {
        // IDs from the database, never path fragments supplied by a caller.
        ensure!(
            job.len() == 32 && job.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid job ID"
        );
        let extension = Path::new(filename)
            .extension()
            .and_then(|x| x.to_str())
            .filter(|e| {
                !e.is_empty() && e.len() <= 10 && e.bytes().all(|b| b.is_ascii_alphanumeric())
            })
            .unwrap_or("bin")
            .to_lowercase();
        let ident = id();
        let relative = PathBuf::from("artifacts")
            .join(job)
            .join(format!("{ident}.{extension}"));
        let path = self.root.join(&relative);
        fs::create_dir_all(path.parent().unwrap())?;
        Ok((ident, path))
    }
    fn register_artifact(
        &self,
        job: &str,
        run: &str,
        ident: &str,
        path: &Path,
        label: &str,
        source: &str,
    ) -> Result<Value> {
        let (hash, size) = Self::hash(path)?;
        ensure!(size <= MAX_ARTIFACT, "Artifact limit is 25 MiB");
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::writable(&Self::row(&tx, "jobs", job)?)?;
        ensure!(
            Self::row(&tx, "runs", run)?["job_id"] == job,
            "Run belongs to another job"
        );
        let a = json!({"id":ident,"job_id":job,"run_id":run,"label":label,"source":source,
            "stored_path":path.strip_prefix(&self.root)?.to_string_lossy().replace('\\',"/"),"size":size,"sha256":hash,"created_at":now()});
        tx.execute(
            "INSERT INTO artifacts VALUES(?1,?2,?3,?4)",
            params![ident, job, run, a.to_string()],
        )?;
        Self::event(&tx, job, "artifact", &a)?;
        tx.commit()?;
        Ok(a)
    }
    pub fn attach_bytes(
        &self,
        job: &str,
        run: &str,
        filename: &str,
        bytes: &[u8],
        label: &str,
    ) -> Result<Value> {
        ensure!(
            bytes.len() as u64 <= MAX_ARTIFACT,
            "Artifact limit is 25 MiB"
        );
        self.run(job, run)?;
        let (ident, path) = self.artifact_path(job, filename)?;
        let result = (|| {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            f.write_all(bytes)?;
            self.register_artifact(job, run, &ident, &path, label, "browser_collected")
        })();
        if result.is_err() {
            let _ = fs::remove_file(&path);
        }
        result
    }
    fn attach(&self, job: &str, run: &str, path: &str, label: &str) -> Result<Value> {
        let j = self.basic(job)?;
        Self::writable(&j)?;
        self.run(job, run)?;
        let source = Path::new(path).canonicalize()?;
        let workspace = Path::new(text(&j, "workspace")?).canonicalize()?;
        ensure!(
            source.starts_with(workspace) && source.is_file(),
            "Artifact must be a file inside this job's workspace"
        );
        let mut input = File::open(&source)?.take(MAX_ARTIFACT + 1);
        let (ident, dest) = self.artifact_path(job, path)?;
        let result = (|| {
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)?;
            ensure!(
                std::io::copy(&mut input, &mut out)? <= MAX_ARTIFACT,
                "Artifact limit is 25 MiB"
            );
            drop(out);
            self.register_artifact(job, run, &ident, &dest, label, "operator_imported")
        })();
        if result.is_err() {
            let _ = fs::remove_file(&dest);
        }
        result
    }
    pub fn execute(&self, name: &str, a: &Value) -> Result<Value> {
        if name == "qa_get_job" {
            return self.snapshot(text(a, "job_id")?);
        }
        if name == "qa_get_events" {
            return self.events(
                text(a, "job_id")?,
                a["after_seq"].as_i64().unwrap_or(0),
                a["limit"].as_u64().unwrap_or(50) as usize,
            );
        }
        if name == "qa_export_report" {
            return self.export(text(a, "job_id")?);
        }
        if name == "qa_attach_artifact" {
            return self.attach(
                text(a, "job_id")?,
                text(a, "run_id")?,
                text(a, "path")?,
                text(a, "label")?,
            );
        }
        if name == "qa_record_step" {
            ensure!(
                !text(a, "action")?.trim().is_empty() && !text(a, "observation")?.trim().is_empty(),
                "Action and observation must be nonempty"
            );
            let job = text(a, "job_id")?;
            let r = self.reserve(
                job,
                text(a, "run_id")?,
                a["action"].clone(),
                "operator_attested",
            )?;
            return self.finish(job, &r, &a["observation"], "observed");
        }
        let mut c = self.connect()?;
        if name == "qa_list_jobs" {
            let mut st = c.prepare("SELECT data FROM jobs ORDER BY rowid DESC LIMIT 500")?;
            let list=st.query_map([],|r|r.get::<_,String>(0))?.map(|r|{let j:Value=serde_json::from_str(&r?)?;
                Ok(json!({"id":j["id"],"title":j["contract"]["title"],"status":j["status"],"build_id":j["contract"]["build_id"]}))}).collect::<Result<Vec<_>>>()?;
            return Ok(json!(list));
        }
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if name == "qa_create_job" {
            let mut contract = a["contract"].clone();
            for (k, v) in [
                ("max_steps", json!(100)),
                ("max_seconds", json!(3600)),
                ("max_reported_cost_usd", json!(10.)),
                ("allowed_origins", json!([])),
            ] {
                if contract.get(k).is_none() {
                    contract[k] = v;
                }
            }
            let criteria = contract["criteria"]
                .as_array()
                .context("Expected criteria")?;
            let ids = criteria
                .iter()
                .map(|x| text(x, "id"))
                .collect::<Result<HashSet<_>>>()?;
            ensure!(ids.len() == criteria.len(), "Criterion IDs must be unique");
            let mut origins = Vec::new();
            for v in strings(&contract["allowed_origins"])? {
                let u = url::Url::parse(&v)?;
                ensure!(
                    u.path() == "/" && u.query().is_none() && u.fragment().is_none(),
                    "Use exact HTTP(S) origins"
                );
                let origin = crate::browser::origin(&v)?;
                if !origins.contains(&origin) {
                    origins.push(origin);
                }
            }
            contract["allowed_origins"] = json!(origins);
            let j = json!({"id":id(),"contract":contract,"workspace":self.workspace,"status":"active","created_at":now(),"steps_used":0,"reported_cost_usd":0.,"checkpoint":null,"outcomes":null});
            tx.execute(
                "INSERT INTO jobs VALUES(?1,?2)",
                params![text(&j, "id")?, j.to_string()],
            )?;
            Self::event(
                &tx,
                text(&j, "id")?,
                "created",
                &json!({"build_id":j["contract"]["build_id"]}),
            )?;
            tx.commit()?;
            return Ok(j);
        }
        let job = text(a, "job_id")?;
        let mut j = Self::row(&tx, "jobs", job)?;
        let result = match name {
            "qa_set_status" => {
                ensure!(
                    (j["status"] == "active" && a["status"] == "paused")
                        || (j["status"] == "paused" && a["status"] == "active"),
                    "Only active→paused or paused→active is allowed"
                );
                j["status"] = a["status"].clone();
                Self::put(&tx, "jobs", &j)?;
                Self::event(&tx, job, "status", &json!({"status":a["status"]}))?;
                j
            }
            "qa_checkpoint" => {
                Self::writable(&j)?;
                let mut p = a["checkpoint"].clone();
                for k in ["explored", "hypotheses", "next_actions"] {
                    if p.get(k).is_none() {
                        p[k] = json!([]);
                    }
                }
                j["checkpoint"] = p.clone();
                Self::put(&tx, "jobs", &j)?;
                Self::event(&tx, job, "checkpoint", &p)?;
                p
            }
            "qa_begin_run" => {
                Self::budget(&j)?;
                ensure!(
                    !text(a, "fixture_description")?.trim().is_empty(),
                    "Describe the actual fixture/reset"
                );
                let r = json!({"id":id(),"job_id":job,"kind":a["kind"],"fixture":a["fixture_description"],"fixture_source":"operator_attested","created_at":now()});
                tx.execute(
                    "INSERT INTO runs VALUES(?1,?2,?3)",
                    params![text(&r, "id")?, job, r.to_string()],
                )?;
                Self::event(&tx, job, "run_started", &r)?;
                r
            }
            "qa_report_cost" => {
                Self::writable(&j)?;
                let cost = a["usd"].as_f64().context("Expected USD number")?;
                ensure!(
                    cost.is_finite()
                        && (0. ..=100000.).contains(&cost)
                        && !text(a, "note")?.trim().is_empty(),
                    "Provide finite nonnegative USD and a note"
                );
                let cost = j["reported_cost_usd"].as_f64().unwrap_or(0.) + cost;
                ensure!(cost.is_finite(), "Cost overflow");
                j["reported_cost_usd"] = json!(cost);
                Self::put(&tx, "jobs", &j)?;
                Self::event(
                    &tx,
                    job,
                    "cost_reported",
                    &json!({"usd":a["usd"],"note":a["note"]}),
                )?;
                json!({"reported_cost_usd":cost,"over_budget":cost>j["contract"]["max_reported_cost_usd"].as_f64().unwrap_or(10.)})
            }
            "qa_add_finding" => {
                Self::active(&j)?;
                let run = text(a, "run_id")?;
                let f = &a["finding"];
                ensure!(
                    j["contract"]["criteria"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|x| x["id"] == f["criterion_id"]),
                    "Unknown criterion"
                );
                self.verify(&tx, job, &strings(&f["artifact_ids"])?, Some(run))?;
                let mut f = f.clone();
                f["id"] = json!(id());
                f["job_id"] = json!(job);
                f["run_id"] = json!(run);
                f["status"] = json!("candidate");
                f["created_at"] = json!(now());
                f["reproductions"] = json!([]);
                tx.execute(
                    "INSERT INTO findings VALUES(?1,?2,?3)",
                    params![text(&f, "id")?, job, f.to_string()],
                )?;
                Self::event(&tx, job, "finding", &f)?;
                f
            }
            "qa_record_reproduction" => {
                Self::active(&j)?;
                let run = text(a, "run_id")?;
                let r = Self::row(&tx, "runs", run)?;
                let mut f = Self::row(&tx, "findings", text(a, "finding_id")?)?;
                ensure!(
                    f["job_id"] == job && r["job_id"] == job,
                    "Finding/run belongs to another job"
                );
                ensure!(
                    r["kind"] == "reproduction" && f["run_id"] != run,
                    "Use a separate reproduction run after resetting the target"
                );
                ensure!(
                    r["created_at"].as_f64() >= f["created_at"].as_f64(),
                    "Reproduction run predates discovery"
                );
                ensure!(
                    !f["reproductions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|x| x["run_id"] == run),
                    "One reproduction result per run"
                );
                let ids = strings(&a["artifact_ids"])?;
                ensure!(
                    !ids.is_empty() && !text(a, "note")?.trim().is_empty(),
                    "Reproduction needs evidence and a note"
                );
                self.verify(&tx, job, &ids, Some(run))?;
                f["reproductions"].as_array_mut().unwrap().push(json!({"run_id":run,"reproduced":a["reproduced"],"artifact_ids":a["artifact_ids"],"note":a["note"]}));
                let reps = f["reproductions"].as_array().unwrap();
                let successes = reps.iter().filter(|x| x["reproduced"] == true).count();
                f["status"] = json!(if successes == reps.len() {
                    "confirmed"
                } else if successes > 0 {
                    "intermittent"
                } else {
                    "not_reproduced"
                });
                Self::put(&tx, "findings", &f)?;
                Self::event(
                    &tx,
                    job,
                    "reproduction",
                    &json!({"finding_id":a["finding_id"],"result":f}),
                )?;
                f
            }
            "qa_reconcile_step" => {
                Self::active(&j)?;
                ensure!(
                    !text(a, "observation")?.trim().is_empty(),
                    "Reconciliation needs an observation"
                );
                let action = text(a, "action_id")?;
                let mut pending = None;
                let mut st=tx.prepare("SELECT kind,data FROM events WHERE job=?1 AND json_extract(data,'$.action_id')=?2 ORDER BY seq")?;
                let mut rows = st.query(params![job, action])?;
                while let Some(row) = rows.next()? {
                    let k: String = row.get(0)?;
                    if k == "step_started" {
                        pending = Some(serde_json::from_str::<Value>(&row.get::<_, String>(1)?)?)
                    } else if k == "step_finished" {
                        pending = None;
                    }
                }
                let mut r = pending.context("Action is unknown or already finished")?;
                r["state"] = json!("unknown");
                r["observation"] = a["observation"].clone();
                r["reconciled"] = json!(true);
                Self::event(&tx, job, "step_finished", &r)?;
                json!({"action_id":action,"state":"unknown","observation":a["observation"]})
            }
            "qa_complete_job" => {
                Self::writable(&j)?;
                let pending:i64=tx.query_row("SELECT COUNT(*) FROM events s WHERE s.job=?1 AND s.kind='step_started' AND NOT EXISTS(SELECT 1 FROM events f WHERE f.job=s.job AND f.kind='step_finished' AND json_extract(f.data,'$.action_id')=json_extract(s.data,'$.action_id'))",[job],|r|r.get(0))?;
                ensure!(pending == 0, "Unfinished actions exist; reconcile first");
                let outcomes = a["outcomes"].as_array().context("Expected outcomes")?;
                let ids = outcomes
                    .iter()
                    .map(|x| text(x, "criterion_id"))
                    .collect::<Result<HashSet<_>>>()?;
                let criteria = j["contract"]["criteria"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| text(x, "id"))
                    .collect::<Result<HashSet<_>>>()?;
                ensure!(
                    ids.len() == outcomes.len() && ids == criteria,
                    "Provide exactly one outcome for every criterion"
                );
                for o in outcomes {
                    let empty = json!([]);
                    let evidence = strings(o.get("artifact_ids").unwrap_or(&empty))?;
                    ensure!(
                        !["pass", "fail"].contains(&text(o, "result")?) || !evidence.is_empty(),
                        "pass/fail needs evidence"
                    );
                    self.verify(&tx, job, &evidence, None)?;
                    if o["result"] == "pass" {
                        let count:i64=tx.query_row("SELECT COUNT(*) FROM findings WHERE job=?1 AND json_extract(data,'$.criterion_id')=?2 AND json_extract(data,'$.status') IN ('candidate','confirmed','intermittent')",params![job,text(o,"criterion_id")?],|r|r.get(0))?;
                        ensure!(
                            count == 0,
                            "A criterion with an unresolved or reproduced finding cannot pass on this build"
                        );
                    }
                }
                j["status"] = json!("completed");
                j["outcomes"] = a["outcomes"].clone();
                j["completed_at"] = json!(now());
                Self::put(&tx, "jobs", &j)?;
                Self::event(&tx, job, "completed", &json!({"outcomes":a["outcomes"]}))?;
                j
            }
            _ => bail!("Unknown tool: {name}"),
        };
        tx.commit()?;
        Ok(result)
    }
    pub fn export(&self, job: &str) -> Result<Value> {
        let c = self.connect()?;
        let mut j = Self::row(&c, "jobs", job)?;
        j.as_object_mut().unwrap().remove("workspace");
        let dir = self.root.join("reports").join(job);
        fs::create_dir_all(&dir)?;
        // Temporary files are atomically persisted only after integrity verification and successful output.
        let mut jf = tempfile::NamedTempFile::new_in(&dir)?;
        let mut mf = tempfile::NamedTempFile::new_in(&dir)?;
        {
            let mut out = BufWriter::with_capacity(65536, &mut jf);
            let mut md = BufWriter::new(&mut mf);
            let mut base = serde_json::to_string(&j)?;
            base.pop();
            write!(out, "{base}")?;
            writeln!(
                md,
                "# {}\n\nBuild: {}\nJob status: {}\n\nEvidence-backed operator assessment; not a guarantee of defect absence.\nExternal resets and observations are operator-attested.\n\nSteps: {}\n\n## Criteria\n",
                text(&j["contract"], "title")?,
                text(&j["contract"], "build_id")?,
                text(&j, "status")?,
                j["steps_used"]
            )?;
            if let Some(outcomes) = j["outcomes"].as_array() {
                for o in outcomes {
                    writeln!(
                        md,
                        "- {}: **{}** — {}",
                        text(o, "criterion_id")?,
                        text(o, "result")?,
                        text(o, "reason")?
                    )?;
                }
            } else {
                writeln!(md, "No final outcomes recorded; this is an unfinished job.")?;
            }
            for table in ["runs", "findings", "artifacts", "events"] {
                write!(out, ",\"{table}\":[")?;
                let query = if table == "events" {
                    "SELECT json_object('seq',seq,'at',at,'kind',kind,'data',json(data)) FROM events WHERE job=?1 ORDER BY seq".to_owned()
                } else {
                    format!("SELECT data FROM {table} WHERE job=?1 ORDER BY rowid")
                };
                let mut st = c.prepare(&query)?;
                let mut rows = st.query([job])?;
                let mut first = true;
                if table == "findings" {
                    writeln!(md, "\n## Findings\n")?;
                }
                if table == "artifacts" {
                    writeln!(md, "\n## Artifact manifest\n")?;
                }
                while let Some(row) = rows.next()? {
                    let raw: String = row.get(0)?;
                    let v: Value = serde_json::from_str(&raw)?;
                    if !first {
                        write!(out, ",")?;
                    }
                    first = false;
                    if table == "events" {
                        serde_json::to_writer(&mut out, &v)?;
                    } else {
                        write!(out, "{raw}")?;
                    }
                    if table == "artifacts" {
                        let ident = text(&v, "id")?;
                        self.verify(&c, job, &[ident.to_owned()], None)?;
                        writeln!(
                            md,
                            "- [{ident}](../../{}) — {}; SHA-256 `{}`",
                            text(&v, "stored_path")?,
                            text(&v, "source")?,
                            text(&v, "sha256")?
                        )?;
                    }
                    if table == "findings" {
                        writeln!(
                            md,
                            "### {}\n\nSeverity: {}; status: {}\nExpected: {}\nActual: {}\n",
                            text(&v, "title")?,
                            text(&v, "severity")?,
                            text(&v, "status")?,
                            text(&v, "expected")?,
                            text(&v, "actual")?
                        )?;
                        for (n, step) in strings(&v["steps"])?.iter().enumerate() {
                            writeln!(md, "{}. {step}", n + 1)?;
                        }
                        let reps = v["reproductions"].as_array().unwrap();
                        writeln!(
                            md,
                            "\nReproductions: {}/{}\n",
                            reps.iter().filter(|r| r["reproduced"] == true).count(),
                            reps.len()
                        )?;
                    }
                }
                write!(out, "]")?;
            }
            writeln!(out, "}}")?;
            out.flush()?;
            md.flush()?;
        }
        let jp = dir.join("report.json");
        let mp = dir.join("report.md");
        jf.persist(&jp)?;
        mf.persist(&mp)?;
        Ok(json!({"json":jp,"markdown":mp,"job_status":j["status"]}))
    }
}
