use crate::{
    config::{self, Config, LoadedWorkflow, Step, Verify},
    provider::Provider,
    store::{Artifact, Session, Store},
    tools::{parse_action, Tools},
    workspace,
};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::Path,
};

const PROTOCOL: &str = r#"You are a human-supervised workflow agent. Do not silently change scope, fix unrelated issues, or use risky workarounds.
Each reply MUST be exactly one YAML mapping (optionally fenced with ```yaml). No JSON, no commentary outside YAML.
Available actions:
  action: tool
  tool: read | write | shell | web
  path: relative/path   # for read/write
  input: command_or_search_query  # for shell/web
  content: |              # for write
    file content
  action: artifact
  name: notes.md
  content: |
    artifact content
  action: deviation
  deviation_id: configured_rule_id
  summary: |
    Evidence, blocker, and proposed options
  action: finish
  summary: |
    Description and evidence that step is complete
Use exactly one action per response. An artifact action registers a session artifact, it does NOT modify code. Each step is one stage of a multi-step workflow with its own fresh context and tools, and `finish` completes only the current step: the harness then advances to the next step, which continues the work. Finish this step as soon as its required outputs are registered, even when the overall session goal is not yet fully achieved, and only request the tools this step enables. Commands and edits require authorization. Shell commands run without network or host HOME access, with project files read-only. Use the native write tool for all source edits; shell writes cannot alter project files. Tool failure is a fact; don't hide it. If a tool fails, describe it instead of working around an environment mismatch.
Any action may include an optional activity_title string, for example: activity_title: Exploring call sites
Use a short plain-text description of intent, 1–80 Unicode characters after trimming surrounding whitespace; no control characters, newlines, invisible direction/format controls, or claims of verified success. Omit it when unnecessary. It is only a model-authored annotation, never permission or execution evidence. Keep exactly one action per response; there is no separate title action.
No invented tool results. If blocked, report a deviation. Keep outputs concise.
"#;

// TUI workers render from recorded events; direct stdout corrupts the screen.
macro_rules! console_println {
    ($engine:expr, $($arg:tt)*) => {
        if !$engine.tools.is_tui() {
            println!($($arg)*);
        }
    };
}

macro_rules! console_eprintln {
    ($engine:expr, $($arg:tt)*) => {
        if !$engine.tools.is_tui() {
            eprintln!($($arg)*);
        }
    };
}

pub struct Engine {
    pub root: std::path::PathBuf,
    pub cfg: Config,
    pub store: Store,
    pub provider: Provider,
    pub tools: Tools,
}
impl Engine {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let cfg = config::load_config(&root)?;
        let store = Store::open(&root)?;
        let provider = Provider::new(cfg.provider.clone());
        let tools = Tools::new(root.clone(), cfg.permissions.clone())?;
        Ok(Self {
            root,
            cfg,
            store,
            provider,
            tools,
        })
    }
    pub async fn run(
        &mut self,
        session_id: &str,
        extra: Option<&str>,
        model_override: Option<&str>,
    ) -> Result<()> {
        let owner = self.store.claim_execution(session_id, extra)?;
        let result = self.run_owned(session_id, extra, model_override).await;
        let cleanup = self.store.finish_execution(session_id, &owner, result.as_ref().err());
        match (result, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error.context("could not release execution ownership; recover-session is required")),
            (Err(error), Err(cleanup)) => Err(error.context(format!("execution cleanup also failed: {cleanup:#}; recover-session is required"))),
        }
    }
    async fn run_owned(
        &mut self,
        session_id: &str,
        extra: Option<&str>,
        model_override: Option<&str>,
    ) -> Result<()> {
        self.store.ensure_no_pending_writes()?;
        let mut s = self.store.session(session_id)?;
        let mut steps = 0usize;
        while s.status != "completed" && steps < 100 {
            let workflow = config::load_workflow(&self.root, &s.workflow)?;
            let step = workflow
                .steps
                .get(&s.current_step)
                .context("current step absent from workflow")?
                .clone();
            let override_prompt = extra.unwrap_or("");
            let resolved_model = model_override
                .or(step.model.as_deref())
                .unwrap_or(&self.cfg.provider.model)
                .to_owned();
            let definition = serde_yaml::to_string(&step)?;
            self.store.begin_attempt(&s, &step.id, &definition, &resolved_model)?;
            s.status = "running".into();
            self.store.save(&s)?;
            console_println!(self, "\n=== {} / {} ===", s.workflow, step.id);
            let before = match workspace::record(
                &self.root, &self.store, &s, &step.id, "before",
            ) {
                Ok(recording) => recording,
                Err(error) => {
                    self.block(
                        &mut s, &format!("workspace recording failed: {error:#}"), &step.id,
                    ).await?;
                    break;
                }
            };
            let outcome = self
                .execute_step(&s, &workflow, &step, override_prompt, &resolved_model)
                .await;
            // Record even failed/blocked attempts, so an external edit cannot
            // disappear from the observed step's workspace history.
            let audit = workspace::record(
                &self.root, &self.store, &s, &step.id, "after",
            ).and_then(|(after_event, after)| {
                let changes = workspace::differences(&before.1, &after);
                if !changes.is_empty() {
                    self.store.event(
                        &s, Some(&step.id), "workspace.delta",
                        &serde_json::to_string(&changes)?,
                    )?;
                }
                let audit=crate::workspace_audit::correlate(&self.store,&s,before.0,after_event,Some(after_event),&before.1,&after)?;
                if !audit.paths.is_empty() || !audit.warnings.is_empty() {
                    self.store.event(&s,Some(&step.id),"workspace.correlation",&serde_json::to_string(&audit)?)?;
                }
                Ok(())
            });
            let outcome = match (outcome, audit) {
                (result, Ok(())) => result,
                (Ok(_), Err(error)) => Err(error.context("workspace recording failed after step")),
                (Err(error), Err(audit_error)) => Err(error.context(
                    format!("workspace recording also failed: {audit_error:#}"),
                )),
            };
            let already_detected = matches!(&outcome, Ok(Outcome::DetectedDeviation(..)));
            match outcome {
                Ok(Outcome::Done(summary)) => {
                    self.store
                        .event(&s, Some(&step.id), "step.completed", &summary)?;
                    console_println!(self, "[complete] {summary}");
                    if let Some(next) = step.transitions.success {
                        s.current_step = next;
                        s.status = "ready".into();
                        self.store.save(&s)?;
                    } else {
                        s.status = "completed".into();
                        self.store.save(&s)?;
                        self.store.event(&s, None, "session.completed", "")?;
                        self.notify(&s, "Session completed").await;
                    }
                }
                Ok(Outcome::Deviation(id, summary)) | Ok(Outcome::DetectedDeviation(id, summary)) => {
                    self.store.event(
                        &s,
                        Some(&step.id),
                        "step.deviation",
                        &format!("{id}: {summary}"),
                    )?;
                    console_println!(self, "[deviation: {id}] {summary}");
                    let rule = step.deviations.iter().find(|d| d.id == id);
                    if !already_detected {
                        if let Some(rule) = rule {
                            match self.detect_deviation(&s, &workflow, &step, rule, &summary, &resolved_model).await {
                                Ok(Some(verdict)) if !verdict.detected => {
                                    self.block(&mut s, &format!("detector rejected deviation {id}: {}", verdict.reason), &step.id).await?;
                                    break;
                                }
                                Err(error) => {
                                    self.store.event(&s, Some(&step.id), "step.error", &format!("{error:#}"))?;
                                    self.block(&mut s, &format!("{error:#}"), &step.id).await?;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    match rule.map(|d| d.action.as_str()) {
                        Some("goto") => {
                            s.current_step = rule.unwrap().target.clone().unwrap();
                            s.status = "ready".into();
                            self.store.save(&s)?;
                        }
                        Some("retry") => {
                            let count = self
                                .store
                                .history(&s)?
                                .iter()
                                .filter(|(_, st, k, data)| {
                                    st == &step.id
                                        && k == "step.deviation"
                                        && data.starts_with(&format!("{id}:"))
                                })
                                .count() as u32;
                            if count <= rule.unwrap().max_retries.unwrap_or(0) {
                                s.status = "ready".into();
                                self.store.save(&s)?;
                            } else {
                                self.block(&mut s, &format!("retry limit reached: {id}"), &step.id)
                                    .await?;
                                break;
                            }
                        }
                        _ => {
                            self.block(&mut s, &summary, &step.id).await?;
                            break;
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    self.store.event(&s, Some(&step.id), "step.error", &msg)?;
                    console_eprintln!(self, "[step error] {msg}");
                    self.block(&mut s, &msg, &step.id).await?;
                    break;
                }
            }
            steps += 1;
        }
        if steps >= 100 {
            self.block(
                &mut s,
                "session reached 100 step-run safety limit",
                "engine",
            )
            .await?;
        }
        console_println!(self, 
            "session {}: {} (branch {})",
            s.id, s.status, s.active_branch
        );
        Ok(())
    }
    async fn detect_deviation(
        &self, s: &Session, wf: &LoadedWorkflow, step: &Step,
        rule: &config::Deviation, summary: &str, model: &str,
    ) -> Result<Option<Detection>> {
        let Some(prompt) = &rule.prompt else { return Ok(None) };
        let instructions = config::render_includes(
            &format!("{{{{ include \"{prompt}\" }}}}"), &self.root, &wf.folder,
        )?;
        let query = format!("Evaluate whether this deviation is supported by available evidence.\nRule: {}\nInstructions: {}\nAgent report: {}\nReturn YAML only:\ndetected: true|false\nreason: explanation", rule.id, instructions, summary);
        let raw = self.model_call(s, &step.id, "deviation-detector", model, &[json!({"role":"user","content":query})])
            .await.context("deviation detector failed")?;
        self.store.event(s, Some(&step.id), "deviation.detector", &raw)?;
        let yaml = raw.trim().trim_start_matches("```yaml").trim_start_matches("```yml")
            .trim_end_matches("```").trim();
        let verdict: Detection = serde_yaml::from_str(yaml).context("invalid deviation detector output")?;
        self.store.event(s, Some(&step.id),
            if verdict.detected { "deviation.accepted" } else { "deviation.rejected" },
            &format!("{}: {}", rule.id, verdict.reason))?;
        Ok(Some(verdict))
    }
    pub(crate) async fn model_call(&self,s:&Session,step:&str,purpose:&str,model:&str,messages:&[serde_json::Value])->Result<String> {
        self.model_call_with_message(s,step,purpose,model,messages,None).await
    }
    pub(crate) async fn model_call_with_message(&self,s:&Session,step:&str,purpose:&str,model:&str,messages:&[serde_json::Value],interaction:Option<(&str,&str)>)->Result<String> {
        self.dispatch_model_call(s,step,purpose,model,messages,interaction).await.context(crate::provider_failure::ModelCallFailed)
    }
    async fn dispatch_model_call(&self,s:&Session,step:&str,purpose:&str,model:&str,messages:&[serde_json::Value],interaction:Option<(&str,&str)>)->Result<String> {
        use crate::provider_failure::{Failure,Kind};
        for attempt in 1..=3 {
            if self.provider.retry_cancel.load(std::sync::atomic::Ordering::SeqCst) {return Err(Failure::local(Kind::Cancelled).into());}
            let discover = true;
            #[cfg(test)]
            let discover = discover && self.provider.script.is_none();
            let metadata = if discover {
                crate::pricing::resolve(&self.root, &self.cfg.provider, model, false).await
            } else {
                self.cfg.provider.model_metadata.get(model).cloned().unwrap_or_default()
            };
            if self.provider.retry_cancel.load(std::sync::atomic::Ordering::SeqCst) {return Err(Failure::local(Kind::Cancelled).into());}
            let provider=reqwest::Url::parse(&self.cfg.provider.base_url).map(|url|url.origin().ascii_serialization()).unwrap_or_else(|_|"unknown provider".into());
            let mut call=crate::usage::Call::new(s,step,purpose,model,&provider,metadata,messages);
            if let Some((message,snapshot))=interaction {call.manager_message_id=Some(message.into());call.manager_snapshot_id=Some(snapshot.into());}
            self.store.begin_model_call(s,&mut call)?;
            let result=self.provider.complete(model,messages).await;
            let failure=match &result {
                Ok(response)=>{call.finish(response);response.failure.clone()},
                Err(error)=>{call.status="failed-or-unknown-billing".into();error.downcast_ref::<Failure>().cloned()},
            };
            if let Some(failure)=&failure {
                call.counts.notes.push(failure.to_string());
                if failure.status.is_none() && matches!(failure.kind,Kind::Credentials|Kind::Request) {call.status="not-sent".into();call.counts.notes.push("No HTTP request was sent; input counts describe the prepared request, not billed usage.".into());}
                if failure.billing_uncertain {call.estimated_cost=None;}
            }
            self.store.finish_model_call(s,&call).context("could not persist model usage; initial request record retained")?;
            if let Some(failure)=failure {
                if purpose!="manager" {console_eprintln!(self,"[provider] {failure}");}
                self.store.event(s,Some(step),"provider.failure",&serde_json::to_string(&serde_json::json!({"call_id":call.id,"purpose":purpose,"request_number":attempt,"failure":failure}))?)?;
                if let Some(delay)=failure.delay(attempt) {
                    if purpose!="manager" {console_eprintln!(self,"[provider retry] request {}/3 in {:.2}s",attempt+1,delay.as_secs_f64());}
                    self.store.event(s,Some(step),"provider.retry",&serde_json::json!({"call_id":call.id,"purpose":purpose,"next_request":attempt+1,"wait_ms":delay.as_millis(),"message":if purpose=="manager" {"Waiting before manager retry; /cancel-manager cancels future manager requests"} else {"Waiting before provider retry; x in TUI cancels future provider requests"}}).to_string())?;
                    if let Err(cancelled)=crate::provider_failure::wait(delay,&self.provider.retry_cancel).await {
                        self.store.event(s,Some(step),"provider.retry-cancelled",&cancelled.to_string())?;
                        return Err(cancelled.into());
                    }
                    continue;
                }
                if failure.retryable {
                    self.store.event(s,Some(step),"provider.retry-stopped","Automatic retry budget exhausted or Retry-After exceeds the 15-second per-wait limit; resume explicitly later.")?;
                    return Err(anyhow::Error::new(failure).context("Automatic retry budget exhausted or requested wait exceeds 15s; resume explicitly later."));
                }
                return Err(failure.into());
            }
            return result?.content.map_err(anyhow::Error::msg);
        }
        unreachable!("last request cannot schedule another retry")
    }

    async fn block(&self, s: &mut Session, reason: &str, step: &str) -> Result<()> {
        s.status = "blocked".into();
        self.store.save(s)?;
        self.store.event(s, Some(step), "step.blocked", reason)?;
        self.notify(s, &format!("Blocked on {step}: {reason}"))
            .await;
        Ok(())
    }
    async fn notify(&self, s: &Session, message: &str) {
        let Some(env_name) = &self.cfg.notifications.discord_webhook_env else {
            return;
        };
        let Ok(url) = std::env::var(env_name) else {
            return;
        };
        // Webhook is deliberately optional; failure is logged, never silently ignored.
        let payload = json!({"content":format!("Greenlight [{}] {}",s.id,message).chars().take(1900).collect::<String>()});
        match self.tools.client.post(url).json(&payload).send().await {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => console_eprintln!(self, "[notification error] Discord HTTP {}", r.status()),
            Err(e) => console_eprintln!(self, "[notification error] {e}"),
        }
    }
    fn select_inputs(&self, s: &Session, wf: &LoadedWorkflow, step: &Step) -> Result<Vec<Artifact>> {
        let last = self.store.history(s)?;
        let previous = last
            .iter()
            .rev()
            .find(|(_, _, k, _)| k == "step.completed")
            .map(|(_, id, _, _)| id.clone());
        let mut selected: BTreeMap<String, Artifact> = BTreeMap::new();
        for inp in &step.inputs.from {
            let source = if inp.source == "previous" {
                previous.as_deref().unwrap_or("__missing__")
            } else {
                &inp.source
            };
            for a in self.store.artifacts(s, source)? {
                if inp.select == "required" {
                    let producing_step = wf
                        .steps
                        .get(&a.step)
                        .context("unknown producing step for artifact")?;
                    if !producing_step.outputs.required.contains(&a.name) {
                        continue;
                    }
                }
                selected.insert(a.name.clone(), a);
            }
        }
        for required in &step.inputs.required {
            if !selected.contains_key(required) {
                for a in self.store.artifacts(s, "session")? {
                    if &a.name == required {
                        selected.insert(a.name.clone(), a);
                    }
                }
            }
            if !selected.contains_key(required) {
                bail!("missing required input artifact {required}");
            }
        }
        Ok(selected.into_values().collect())
    }
    fn enforce_input_approvals(&self, s: &Session, wf: &LoadedWorkflow, step: &Step, inputs: &[Artifact]) -> Result<Vec<i64>> {
        let mut approvals = std::collections::BTreeSet::new();
        if step.kind == "human" { return Ok(Vec::new()) }
        for gate in wf.steps.values().filter(|gate| gate.kind == "human") {
            let mut pending = vec![gate.id.as_str()];
            let mut seen = std::collections::HashSet::new();
            let mut downstream = false;
            while let Some(id) = pending.pop() {
                if !seen.insert(id) { continue; }
                if id != gate.id && id == step.id { downstream = true; break; }
                if let Some(node) = wf.steps.get(id) {
                    if let Some(next) = node.transitions.success.as_deref() { pending.push(next); }
                    pending.extend(node.deviations.iter().filter(|d| d.action == "goto").filter_map(|d| d.target.as_deref()));
                }
            }
            if downstream {
                let mut protected = self.store.gate_input_names(s, &gate.id)?;
                protected.extend(gate.inputs.required.iter().cloned());
                for artifact in inputs.iter().filter(|a| protected.contains(&a.name)) {
                    approvals.insert(self.store.require_approved_input(s, &gate.id, artifact)?);
                }
            }
        }
        Ok(approvals.into_iter().collect())
    }
    async fn execute_step(
        &mut self,
        s: &Session,
        wf: &LoadedWorkflow,
        step: &Step,
        extra: &str,
        model: &str,
    ) -> Result<Outcome> {
        let inputs = self.select_inputs(s, wf, step)?;
        let approvals = self.enforce_input_approvals(s, wf, step, &inputs)?;
        let input_manifest = self.store.record_approved_input_manifest(s, &step.id, &inputs, approvals)?;
        match step.kind.as_str() {
            "human" => {
                let prompt = config::render_includes(&step.prompt, &self.root, &wf.folder)?;
                console_println!(self, "{prompt}\n{extra}");
                // Approval must include the actual artifacts, not merely a filename.
                let mut approval = format!("{prompt}\n{extra}");
                for artifact in &inputs {
                    let text = format!("\n--- {} (event #{}) ---\n{}\n--- end {} ---", artifact.name, artifact.event_id, artifact.content, artifact.name);
                    console_println!(self, "{text}");
                    approval.push_str(&text);
                }
                let question = if self.tools.is_tui() {
                    format!("{approval}\n\nApprove and continue?")
                } else {
                    "Approve and continue?".to_owned()
                };
                if !self.tools.ask_yes(&question)? {
                    self.store.event(s, Some(&step.id), "human.declined", "")?;
                    return Ok(Outcome::Deviation(
                        "human_declined".into(),
                        "User declined".into(),
                    ));
                }
                self.store.approve_inputs(s, &step.id, input_manifest, &inputs, || self.select_inputs(s, wf, step))?;
                if let Err(e) = self.verify(s, wf, step, model).await {
                    if crate::provider_failure::is_fatal_model_error(&e) {return Err(e);}
                    return Ok(Outcome::Deviation(
                        "verification_failed".into(),
                        format!("{e:#}"),
                    ));
                }
                Ok(Outcome::Done("human approval granted".into()))
            }
            "command" => {
                let cmd = step.command.as_deref().context("command missing")?;
                let action=crate::tools::AgentAction{action:"tool".into(),activity_title:None,tool:Some("shell".into()),input:Some(cmd.into()),path:None,content:None,name:None,summary:None,deviation_id:None};
                let request=self.store.event(s,Some(&step.id),"tool.request",&serde_yaml::to_string(&action)?)?;
                let execution=self.tools.shell(cmd).await;
                self.log_permissions(s,step)?;
                let result=match execution {
                    Ok(result)=>{self.store.tool_outcome(s,&step.id,request,"shell","tool.result",&result)?;result},
                    Err(error)=>{self.store.tool_outcome(s,&step.id,request,"shell","tool.error",&format!("{error:#}"))?;return Err(error);},
                };
                console_println!(self, "{result}");
                if !result.starts_with("exit_code: 0\n") {
                    return Ok(Outcome::Deviation("command_failed".into(), result));
                }
                if let Err(e) = self.verify(s, wf, step, model).await {
                    if crate::provider_failure::is_fatal_model_error(&e) {return Err(e);}
                    return Ok(Outcome::Deviation(
                        "verification_failed".into(),
                        format!("{e:#}"),
                    ));
                }
                Ok(Outcome::Done("command succeeded".into()))
            }
            "agent" => self.agent(s, wf, step, extra, model, &inputs).await,
            _ => bail!("unsupported step kind"),
        }
    }
    async fn agent(
        &mut self,
        s: &Session,
        wf: &LoadedWorkflow,
        step: &Step,
        extra: &str,
        model: &str,
        inputs: &[Artifact],
    ) -> Result<Outcome> {
        let prompt = config::render_includes(&step.prompt, &self.root, &wf.folder)?;
        let mut context=format!("Session goal:\n{}\n\nStep instructions:\n{}\n\nProject instructions:\n{}\n\nUser instructions:\n{}\n",s.goal,prompt,
            project_instructions(&self.root)?,extra);
        for artifact in inputs {
            context.push_str(&format!("\n--- ARTIFACT {} (event #{}) ---\n{}\n", artifact.name, artifact.event_id, artifact.content));
        }
        if context.len() > self.cfg.session.max_artifact_bytes {
            bail!("assembled step context exceeds byte limit; revise artifact selection");
        }
        let default_tools = vec!["read".to_string(), "shell".to_string(), "write".to_string()];
        let tools = step.tools.as_deref().unwrap_or(&default_tools);
        context.push_str(&format!(
            "\nEnabled tools: {}. Only use these tool names.\n",
            tools.join(", ")
        ));
        self.store
            .event(s, Some(&step.id), "context.assembled", &context)?;
        let mut messages = vec![
            json!({"role":"system","content":PROTOCOL}),
            json!({"role":"user","content":context}),
        ];
        // Continue until the model finishes, reports a deviation, or execution fails/stops.
        // Legacy max_turns settings are accepted for compatibility, not enforced.
        loop {
            let raw = self.model_call(s, &step.id, "coding", model, &messages).await?;
            let model_event = self.store.event(s, Some(&step.id), "model.output", &raw)?;
            // An in-flight response cannot authorize work against an input
            // version that changed while the model was running.
            self.enforce_input_approvals(s, wf, step, &self.select_inputs(s, wf, step)?)?;
            messages.push(json!({"role":"assistant","content":raw}));
            let action = match parse_action(&raw) {
                Ok(a) => a,
                Err(e) => {
                    let result = format!(
                        "Invalid YAML action: {e:#}. Re-emit exactly one valid YAML action."
                    );
                    self.store
                        .event(s, Some(&step.id), "protocol.error", &result)?;
                    messages.push(json!({"role":"user","content":result}));
                    continue;
                }
            };
            if let Some(title) = &action.activity_title {
                let annotation = crate::activity_titles::Annotation {
                    model_event,
                    title: title.clone(),
                };
                self.store.event(
                    s,
                    Some(&step.id),
                    "activity.title",
                    &serde_json::to_string(&annotation)?,
                )?;
            }
            match action.action.as_str() {
                "tool" => {
                    let kind = action.tool.as_deref().context("tool action missing tool")?;
                    let request_event=self.store.event(
                        s,
                        Some(&step.id),
                        "tool.request",
                        &serde_yaml::to_string(&action)?,
                    )?;
                    // A tool that is unavailable, not permitted or that fails is
                    // reported back to the model as a fact so it can adapt — pick
                    // an enabled tool, register an artifact, or report a
                    // deviation — instead of aborting the whole step.
                    let execution: Result<String> = if tools.iter().any(|t| t == kind) {
                        let result = self.tools.call_journaled(&action, &self.store, s, &step.id).await;
                        self.log_permissions(s, step)?;
                        // An uncertain write must stop execution, not become
                        // a retryable tool error in the model conversation.
                        self.store.ensure_no_pending_writes()?;
                        result
                    } else {
                        Err(anyhow!(
                            "tool {kind} is not enabled for this step (enabled: {})",
                            tools.join(", ")
                        ))
                    };
                    match execution {
                        Ok(result) => {
                            console_println!(self, 
                                "[tool {kind}] {}",
                                result.chars().take(350).collect::<String>()
                            );
                            self.store
                                .tool_outcome(s,&step.id,request_event,kind,"tool.result",&result)?;
                            messages.push(
                                json!({"role":"user","content":format!("Tool result:\n{result}")}),
                            );
                        }
                        Err(e) => {
                            let error = format!("{e:#}");
                            console_println!(self, "[tool error] {error}");
                            self.store.tool_outcome(s,&step.id,request_event,kind,"tool.error",&error)?;
                            messages.push(json!({"role":"user","content":format!(
                                "Tool error:\n{error}\n\nThe tool did not run; do not invent its output. If this step enables another tool that can achieve the same goal, use it; if this step's required outputs are already registered, reply with a finish action; otherwise report one of this step's declared deviations."
                            )}));
                        }
                    }
                }
                "artifact" => {
                    let name = action.name.as_deref().context("artifact missing name")?;
                    let content = action
                        .content
                        .as_deref()
                        .context("artifact missing content")?;
                    if content.len() > self.cfg.session.max_artifact_bytes {
                        bail!("artifact exceeds configured size limit");
                    }
                    self.store.add_artifact(s, &step.id, name, content)?;
                    console_println!(self, "[artifact] {name}");
                    messages
                        .push(json!({"role":"user","content":format!("Artifact {name} stored.")}));
                }

                "deviation" => {
                    let id = action
                        .deviation_id
                        .as_deref()
                        .context("deviation missing deviation_id")?
                        .to_owned();

                    let summary = action.summary.unwrap_or_default();

                    let Some(rule) = step.deviations.iter().find(|d| d.id == id) else {
                        return Ok(Outcome::Deviation(
                            "unconfigured_deviation".into(),
                            format!("{id}: {summary}"),
                        ));
                    };

                    if let Some(verdict) = self.detect_deviation(s, wf, step, rule, &summary, model).await? {
                        if !verdict.detected {
                            messages.push(json!({
                                "role": "user",
                                "content": format!(
                                    "Deviation '{id}' was rejected by the \
                                     independent detector: {}\n\
                                     Continue the current step. Do not report \
                                     this deviation again without new evidence. \
                                     If the required outputs are ready, \
                                     respond with action: finish.",
                                    verdict.reason
                                )
                            }));

                            continue;
                        }
                    }

                    return Ok(Outcome::DetectedDeviation(id, summary));
                }

                "finish" => {
                    let artifacts = self.store.current_run_artifacts(s, &step.id)?;
                    for needed in &step.outputs.required {
                        if !artifacts.contains(needed) {
                            return Ok(Outcome::Deviation(
                                "missing_artifact".into(),
                                format!("required output missing: {needed}"),
                            ));
                        }
                    }
                    if let Err(e) = self.verify(s, wf, step, model).await {
                        if crate::provider_failure::is_fatal_model_error(&e) {return Err(e);}
                        return Ok(Outcome::Deviation(
                            "verification_failed".into(),
                            format!("{e:#}"),
                        ));
                    }
                    return Ok(Outcome::Done(action.summary.unwrap_or_default()));
                }
                _ => bail!("unexpected action"),
            }
        }
    }
    async fn verify(
        &mut self,
        s: &Session,
        wf: &LoadedWorkflow,
        step: &Step,
        model: &str,
    ) -> Result<()> {
        let checks = step.verification.as_ref().context("missing verification")?;
        // Iterative traversal keeps nested AND composites straightforward.
        let mut stack: Vec<&Verify> = vec![checks];
        while let Some(check) = stack.pop() {
            match check {
                Verify::None => {
                    self.store
                        .event(s, Some(&step.id), "verification.none", "")?;
                }
                Verify::All { checks } => {
                    if checks.is_empty() {
                        bail!("verification all must have at least one check");
                    }
                    for c in checks.iter().rev() {
                        stack.push(c);
                    }
                }
                Verify::Any { checks } => {
                    if checks.is_empty() {
                        bail!("verification any must have at least one check");
                    }
                    let mut passed = false;
                    for c in checks {
                        match self.verify_leaf(s, wf, step, model, c).await {
                            Ok(())=>{passed=true;break;}
                            Err(error) if crate::provider_failure::is_fatal_model_error(&error)=>return Err(error),
                            Err(_)=>{} // Ordinary rejected checks retain the configured any-of behavior.
                        }
                    }
                    if !passed {
                        bail!("all verification alternatives failed");
                    }
                }
                other => self.verify_leaf(s, wf, step, model, other).await?,
            }
        }
        self.store
            .event(s, Some(&step.id), "verification.passed", "")?;
        Ok(())
    }
    fn log_permissions(&mut self, s: &Session, step: &Step) -> Result<()> {
        for decision in self.tools.decisions.drain(..) {
            self.store
                .event(s, Some(&step.id), "permission.decision", &decision)?;
        }
        Ok(())
    }
    async fn verify_leaf(
        &mut self,
        s: &Session,
        wf: &LoadedWorkflow,
        step: &Step,
        model: &str,
        check: &Verify,
    ) -> Result<()> {
        let result: Result<()> = match check {
            Verify::None => Ok(()),
            Verify::Human => {
                let approved = self.tools.ask_yes("Verification: approve step result?")?;
                self.store.event(
                    s,
                    Some(&step.id),
                    "verification.human",
                    if approved { "approved" } else { "declined" },
                )?;
                if approved {
                    Ok(())
                } else {
                    bail!("human verification declined")
                }
            }
            Verify::Command { run } => {
                let execution = self.tools.shell(run).await;
                self.log_permissions(s, step)?;
                let output = execution?;
                self.store
                    .event(s, Some(&step.id), "verification.command", &output)?;
                if output.starts_with("exit_code: 0\n") {
                    Ok(())
                } else {
                    bail!("command verification failed: {output}")
                }
            }
            Verify::Agent { prompt } => {
                let full = config::render_includes(
                    &format!("{{{{ include \"{}\" }}}}", prompt),
                    &self.root,
                    &wf.folder,
                )?;

                // Select only versions produced by this durable attempt.
                let (attempt, start) = self.store.active_attempt(s, &step.id)?;
                let step_artifacts = self.store.attempt_artifacts(s, &step.id)?;
                let artifact_evidence = step_artifacts
                    .iter()
                    .map(|a| format!("## {} (artifact event #{})\n{}", a.name, a.event_id, a.content))
                    .collect::<Vec<_>>()
                    .join("\n\n");

                // Keep full recorded bodies from the current attempt only.
                let history = self.store.history(s)?;
                let tool_results = history
                    .iter()
                    .rev()
                    .filter_map(|(event_id, step_id, event_type, data)| {
                        if *event_id > start && step_id == &step.id && matches!(event_type.as_str(), "tool.result" | "tool.error" | "verification.command") {
                            // Preserve failure details beyond the first lines.
                            let preview = data;
                            Some(format!(
                                "### Event #{} from step '{}'\n{}",
                                event_id, step_id, preview
                            ))
                        } else {
                            None
                        }
                    })
                    .take(10) // Limit to last 10 relevant tool results
                    .collect::<Vec<_>>()
                    .join("\n\n");

                // Combine artifacts and tool results
                let evidence = format!("{}\n\n{}", artifact_evidence, tool_results);

                if evidence.len() > self.cfg.session.max_artifact_bytes {
                    bail!("verification evidence exceeds configured byte limit; reduce current attempt outputs");
                }
                let event_ids: Vec<i64> = history.iter().rev().filter(|(id, st, kind, _)|
                    *id > start && st == &step.id && matches!(kind.as_str(), "tool.result" | "tool.error" | "verification.command"))
                    .take(10).map(|(id, _, _, _)| *id).collect();
                self.store.event(s, Some(&step.id), "verification.evidence", &json!({
                    "attempt": attempt,
                    "artifact_events": step_artifacts.iter().map(|a| a.event_id).collect::<Vec<_>>(),
                    "observation_events": event_ids,
                    "selection": "current attempt artifacts and latest 10 observations, full recorded bodies"
                }).to_string())?;
                let query=format!("{}\n\nGoal: {}\nStep: {}\nEvidence:\n{}\nRespond only YAML:\npassed: true|false\nreason: concise explanation", full, s.goal, step.id, evidence);
                let raw = self
                    .model_call(s, &step.id, "verification", model, &[json!({"role":"user","content":query})])
                    .await?;
                self.store
                    .event(s, Some(&step.id), "verification.agent", &raw)?;
                #[derive(Deserialize)]
                struct Verdict {
                    passed: bool,
                    reason: String,
                }
                let raw = raw
                    .trim()
                    .trim_start_matches("```yaml")
                    .trim_start_matches("```yml")
                    .trim_end_matches("```")
                    .trim();
                let verdict: Verdict = serde_yaml::from_str(raw)?;
                if verdict.passed {
                    Ok(())
                } else {
                    bail!("agent reviewer rejected: {}", verdict.reason)
                }
            }
            Verify::All { .. } | Verify::Any { .. } => {
                bail!("nested composite verification is not supported inside any")
            }
        };
        if let Err(e) = &result {
            self.store
                .event(s, Some(&step.id), "verification.failed", &format!("{e:#}"))?;
        }
        result
    }
}

#[derive(Deserialize)]
struct Detection {
    detected: bool,
    reason: String,
}

enum Outcome {
    Done(String),
    Deviation(String, String),
    DetectedDeviation(String, String),
}

fn project_instructions(root: &Path) -> Result<String> {
    let current = root.join(crate::paths::PROJECT_DIR).join("instructions.md");
    match std::fs::read_to_string(&current) {
        Ok(instructions) => Ok(instructions),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let legacy = root.join(crate::paths::LEGACY_PROJECT_DIR).join("instructions.md");
            match std::fs::read_to_string(&legacy) {
                Ok(instructions) => Ok(instructions),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                Err(error) => Err(error).with_context(|| format!("reading {}", legacy.display())),
            }
        }
        Err(error) => Err(error).with_context(|| format!("reading {}", current.display())),
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[tokio::test]
    async fn agent_verification_requires_its_prompt_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("greenlight.yaml"), "provider:\n  model: fixture\n  base_url: http://127.0.0.1:9\n  api_key_env: GREENLIGHT_TEST_UNUSED_KEY\n").unwrap();
        let folder = root.path().join("workflows/demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("workflow.yaml"), "id: demo\ntitle: Demo\nstart: verify\nsteps: [verify]\n").unwrap();
        std::fs::write(folder.join("verify.md"), "---\nid: verify\nverification:\n  type: agent\n  prompt: missing-review.md\n---\nReview\n").unwrap();
        let workflow = config::load_workflow(root.path(), "demo").unwrap();
        let step = &workflow.steps["verify"];
        let mut engine = Engine::open(root.path()).unwrap();
        let session = engine.store.create("demo", "fixture", "verify").unwrap();
        let error = engine.verify_leaf(&session, &workflow, step, "fixture", step.verification.as_ref().unwrap()).await.unwrap_err();
        assert!(format!("{error:#}").contains("missing include missing-review.md"), "{error:#}");
    }

    #[test]
    fn project_instructions_accept_legacy_with_new_precedence() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".stepwright")).unwrap();
        std::fs::write(root.path().join(".stepwright/instructions.md"), "old instructions").unwrap();
        assert_eq!(project_instructions(root.path()).unwrap(), "old instructions");
        std::fs::create_dir(root.path().join(".greenlight")).unwrap();
        std::fs::write(root.path().join(".greenlight/instructions.md"), "new instructions").unwrap();
        assert_eq!(project_instructions(root.path()).unwrap(), "new instructions");
    }
}

#[cfg(test)]
#[path = "provider_engine_tests.rs"]
mod provider_failure_tests;
