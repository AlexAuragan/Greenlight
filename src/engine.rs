use crate::{
    config::{self, Config, LoadedWorkflow, Step, Verify},
    provider::Provider,
    store::{Session, Store},
    tools::{parse_action, Tools},
};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    io::{self, Write},
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
Use exactly one action per response. An artifact action registers a session artifact, it does NOT modify code. Each step is one stage of a multi-step workflow with its own fresh context and tools, and `finish` completes only the current step: the harness then advances to the next step, which continues the work. Finish this step as soon as its required outputs are registered, even when the overall session goal is not yet fully achieved, and only request the tools this step enables. Commands and edits require authorization. Tool failure is a fact; don't hide it. If a tool fails, describe it instead of working around an environment mismatch.
No invented tool results. If blocked, report a deviation. Keep outputs concise.
"#;

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
        let tools = Tools::new(root.clone(), cfg.permissions.clone());
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
            self.store
                .event(&s, Some(&step.id), "step.definition", &definition)?;
            self.store.event(
                &s,
                Some(&step.id),
                "step.started",
                &format!("model: {resolved_model}"),
            )?;
            s.status = "running".into();
            self.store.save(&s)?;
            println!("\n=== {} / {} ===", s.workflow, step.id);
            let outcome = self
                .execute_step(&s, &workflow, &step, override_prompt, &resolved_model)
                .await;
            match outcome {
                Ok(Outcome::Done(summary)) => {
                    self.store
                        .event(&s, Some(&step.id), "step.completed", &summary)?;
                    println!("[complete] {summary}");
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
                Ok(Outcome::Deviation(id, summary)) => {
                    self.store.event(
                        &s,
                        Some(&step.id),
                        "step.deviation",
                        &format!("{id}: {summary}"),
                    )?;
                    println!("[deviation: {id}] {summary}");
                    let rule = step.deviations.iter().find(|d| d.id == id);
                    if let Some(rule) = rule {
                        if let Some(prompt) = &rule.prompt {
                            let check = config::render_includes(
                                &format!("{{{{ include \"{prompt}\" }}}}"),
                                &self.root,
                                &workflow.folder,
                            )?;
                            let query=format!("Evaluate whether this deviation is supported by available evidence.\nRule: {}\nInstructions: {}\nAgent report: {}\nReturn YAML only: detected: true|false\nreason: explanation",id,check,summary);
                            let verdict = self
                                .provider
                                .complete(
                                    &resolved_model,
                                    &[json!({"role":"user","content":query})],
                                )
                                .await;
                            match verdict {
                                Ok(raw) => {
                                    self.store.event(
                                        &s,
                                        Some(&step.id),
                                        "deviation.detector",
                                        &raw,
                                    )?;
                                    #[derive(Deserialize)]
                                    struct Detection {
                                        detected: bool,
                                        reason: String,
                                    }
                                    let raw = raw
                                        .trim()
                                        .trim_start_matches("```yaml")
                                        .trim_start_matches("```yml")
                                        .trim_end_matches("```")
                                        .trim();
                                    match serde_yaml::from_str::<Detection>(raw) {
                                        Ok(v) if v.detected => {
                                            println!("[deviation validated] {}", v.reason)
                                        }
                                        Ok(v) => {
                                            self.block(
                                                &mut s,
                                                &format!(
                                                    "detector rejected deviation {id}: {}",
                                                    v.reason
                                                ),
                                                &step.id,
                                            )
                                            .await?;
                                            break;
                                        }
                                        Err(e) => {
                                            self.block(
                                                &mut s,
                                                &format!("invalid deviation detector output: {e}"),
                                                &step.id,
                                            )
                                            .await?;
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.block(
                                        &mut s,
                                        &format!("deviation detector failed: {e:#}"),
                                        &step.id,
                                    )
                                    .await?;
                                    break;
                                }
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
                    eprintln!("[step error] {msg}");
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
        println!(
            "session {}: {} (branch {})",
            s.id, s.status, s.active_branch
        );
        Ok(())
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
        let payload = json!({"content":format!("Stepwright [{}] {}",s.id,message).chars().take(1900).collect::<String>()});
        match self.tools.client.post(url).json(&payload).send().await {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => eprintln!("[notification error] Discord HTTP {}", r.status()),
            Err(e) => eprintln!("[notification error] {e}"),
        }
    }
    async fn execute_step(
        &mut self,
        s: &Session,
        wf: &LoadedWorkflow,
        step: &Step,
        extra: &str,
        model: &str,
    ) -> Result<Outcome> {
        let known = self.store.artifacts(s, "session")?;
        for required in &step.inputs.required {
            if !known.iter().any(|a| &a.name == required) {
                bail!("missing required input artifact {required}");
            }
        }
        match step.kind.as_str() {
            "human" => {
                let prompt = config::render_includes(&step.prompt, &self.root, &wf.folder)?;
                println!("{prompt}\n{extra}");
                // Approval must include the actual artifacts, not merely a filename.
                let available = self.store.artifacts(s, "session")?;
                for name in &step.inputs.required {
                    let artifact = available.iter().find(|a| &a.name == name)
                        .with_context(|| format!("required approval artifact {name} missing"))?;
                    println!("\n--- {name} ---\n{}\n--- end {name} ---", artifact.content);
                }
                if !ask_yes("Approve and continue?")? {
                    self.store.event(s, Some(&step.id), "human.declined", "")?;
                    return Ok(Outcome::Deviation(
                        "human_declined".into(),
                        "User declined".into(),
                    ));
                }
                self.store.event(s, Some(&step.id), "human.approved", "")?;
                if let Err(e) = self.verify(s, wf, step, model).await {
                    return Ok(Outcome::Deviation(
                        "verification_failed".into(),
                        format!("{e:#}"),
                    ));
                }
                Ok(Outcome::Done("human approval granted".into()))
            }
            "command" => {
                let cmd = step.command.as_deref().context("command missing")?;
                let execution = self.tools.shell(cmd).await;
                self.log_permissions(s, step)?;
                let result = execution?;
                self.store
                    .event(s, Some(&step.id), "tool.result", &result)?;
                println!("{result}");
                if !result.starts_with("exit_code: 0\n") {
                    return Ok(Outcome::Deviation("command_failed".into(), result));
                }
                if let Err(e) = self.verify(s, wf, step, model).await {
                    return Ok(Outcome::Deviation(
                        "verification_failed".into(),
                        format!("{e:#}"),
                    ));
                }
                Ok(Outcome::Done("command succeeded".into()))
            }
            "agent" => self.agent(s, wf, step, extra, model).await,
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
    ) -> Result<Outcome> {
        let prompt = config::render_includes(&step.prompt, &self.root, &wf.folder)?;
        let mut context=format!("Session goal:\n{}\n\nStep instructions:\n{}\n\nProject instructions:\n{}\n\nUser instructions:\n{}\n",s.goal,prompt,
            project_instructions(&self.root)?,extra);
        let last = self.store.history(s)?;
        let previous = last
            .iter()
            .rev()
            .find(|(_, _, k, _)| k == "step.completed")
            .map(|(_, id, _, _)| id.clone());
        let mut selected: HashMap<String, String> = HashMap::new();
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
                selected.insert(a.name.clone(), a.content);
            }
        }
        for required in &step.inputs.required {
            if !selected.contains_key(required) {
                for a in self.store.artifacts(s, "session")? {
                    if &a.name == required {
                        selected.insert(a.name.clone(), a.content);
                    }
                }
            }
            if !selected.contains_key(required) {
                bail!("missing required input artifact {required}");
            }
        }
        for (name, content) in selected {
            context.push_str(&format!("\n--- ARTIFACT {name} ---\n{content}\n"));
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
        let limit = step
            .max_turns
            .unwrap_or(self.cfg.session.max_turns_per_attempt);
        for turn in 0..limit {
            let raw = self.provider.complete(model, &messages).await?;
            self.store.event(s, Some(&step.id), "model.output", &raw)?;
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
            match action.action.as_str() {
                "tool" => {
                    let kind = action.tool.as_deref().context("tool action missing tool")?;
                    self.store.event(
                        s,
                        Some(&step.id),
                        "tool.request",
                        &serde_yaml::to_string(&action)?,
                    )?;
                    // A tool that is unavailable, not permitted or that fails is
                    // reported back to the model as a fact so it can adapt — pick
                    // an enabled tool, register an artifact, or report a
                    // deviation — instead of aborting the whole step. The turn
                    // budget still bounds how often this can happen.
                    let execution: Result<String> = if tools.iter().any(|t| t == kind) {
                        let result = self.tools.call(&action).await;
                        self.log_permissions(s, step)?;
                        result
                    } else {
                        Err(anyhow!(
                            "tool {kind} is not enabled for this step (enabled: {})",
                            tools.join(", ")
                        ))
                    };
                    match execution {
                        Ok(result) => {
                            if let Some(edit) = self.tools.pending_edit.take() {
                                let edit_id = self.store.add_change(
                                    s, &step.id, &edit.path, edit.before.as_deref(), &edit.after
                                )?;
                                println!("[edit recorded] #{edit_id} {}", edit.path);
                            }
                            println!(
                                "[tool {kind}] {}",
                                result.chars().take(350).collect::<String>()
                            );
                            self.store
                                .event(s, Some(&step.id), "tool.result", &result)?;
                            messages.push(
                                json!({"role":"user","content":format!("Tool result:\n{result}")}),
                            );
                        }
                        Err(e) => {
                            let error = format!("{e:#}");
                            println!("[tool error] {error}");
                            self.store.event(s, Some(&step.id), "tool.error", &error)?;
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
                    println!("[artifact] {name}");
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

                    if let Some(prompt) = &rule.prompt {
                        let check = config::render_includes(
                            &format!("{{{{ include \"{prompt}\" }}}}"),
                            &self.root,
                            &wf.folder,
                        )?;

                        let query = format!(
                            "Evaluate whether this deviation is supported \
                             by the available evidence.\n\
                             Rule: {id}\n\
                             Instructions: {check}\n\
                             Agent report: {summary}\n\
                             Return YAML only:\n\
                             detected: true|false\n\
                             reason: explanation"
                        );

                        let raw = self
                            .provider
                            .complete(model, &[json!({"role": "user", "content": query})])
                            .await?;

                        self.store
                            .event(s, Some(&step.id), "deviation.detector", &raw)?;

                        #[derive(Deserialize)]
                        struct Detection {
                            detected: bool,
                            reason: String,
                        }

                        let yaml = raw
                            .trim()
                            .trim_start_matches("```yaml")
                            .trim_start_matches("```yml")
                            .trim_end_matches("```")
                            .trim();

                        let verdict: Detection = serde_yaml::from_str(yaml)
                            .context("invalid deviation detector output")?;

                        if !verdict.detected {
                            self.store.event(
                                s,
                                Some(&step.id),
                                "deviation.rejected",
                                &format!("{id}: {}", verdict.reason),
                            )?;

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

                    return Ok(Outcome::Deviation(id, summary));
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
                        return Ok(Outcome::Deviation(
                            "verification_failed".into(),
                            format!("{e:#}"),
                        ));
                    }
                    return Ok(Outcome::Done(action.summary.unwrap_or_default()));
                }
                _ => bail!("unexpected action"),
            }
            if turn + 1 == limit {
                return Ok(Outcome::Deviation(
                    "turn_budget_exceeded".into(),
                    format!("step hit limit of {limit} model turns"),
                ));
            }
        }
        Ok(Outcome::Deviation(
            "turn_budget_exceeded".into(),
            "max turns reached".into(),
        ))
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
                        if self.verify_leaf(s, wf, step, model, c).await.is_ok() {
                            passed = true;
                            break;
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
                let approved = ask_yes("Verification: approve step result?")?;
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
                    &format!("{{include \"{}\"}}", prompt),
                    &self.root,
                    &wf.folder,
                )?;

                // Collect artifacts from current step
                let step_artifacts = self.store.artifacts(s, &step.id)?;
                let artifact_evidence = step_artifacts
                    .iter()
                    .map(|a| format!("## {}\n{}", a.name, a.content))
                    .collect::<Vec<_>>()
                    .join("\n\n");

                // Collect relevant tool execution history from recent steps
                let history = self.store.history(s)?;
                let tool_results = history
                    .iter()
                    .rev()
                    .filter_map(|(_, step_id, event_type, data)| {
                        if event_type == "tool.result" {
                            // Extract just the first few lines to keep context manageable
                            let preview = data.lines().take(10).collect::<Vec<_>>().join("\n");
                            Some(format!(
                                "### Tool result from step '{}'\n{}",
                                step_id, preview
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

                let query=format!("{}\n\nGoal: {}\nStep: {}\nEvidence:\n{}\nRespond only YAML:\npassed: true|false\nreason: concise explanation", full, s.goal, step.id, evidence);
                let raw = self
                    .provider
                    .complete(model, &[json!({"role":"user","content":query})])
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

fn ask_yes(message: &str) -> Result<bool> {
    println!("{message} [y/N]");
    print!("> ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("y"))
}

enum Outcome {
    Done(String),
    Deviation(String, String),
}

fn project_instructions(root: &Path) -> Result<String> {
    let path = root.join(".stepwright").join("instructions.md");
    if path.exists() {
        Ok(std::fs::read_to_string(path)?)
    } else {
        Ok(String::new())
    }
}
