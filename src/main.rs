mod action_envelope;
mod activity_cards;
mod activity_titles;
mod agent_protocol;
mod anchored_fs;
mod anchored_sqlite;
mod approvals;
mod artifact_history;
mod artifact_review;
mod changes;
mod check_dependencies;
mod config;
mod continuation;
mod conversation_dump;
mod credentials;
#[cfg(test)]
mod deviation_tests;
mod edit_groups;
mod engine;
mod file_discovery;
#[cfg(test)]
mod fs_race_tests;
mod input_pins;
mod locks;
mod manager;
mod manager_delivery;
mod manager_inbox;
mod markdown;
mod model_budget;
mod paths;
mod pricing;
mod project;
mod project_execution;
mod project_inspection;
mod provider;
mod provider_diagnostic;
mod provider_failure;
mod resource_limits;
mod restore;
mod sandbox;
mod sandbox_job;
mod sandbox_validation;
mod session_execution;
mod session_picker;
mod store;
mod summarizer;
mod tool_rerun;
mod tools;
mod tui;
mod tui_actions;
mod usage;
mod verification_export;
mod verification_workspace;
mod workspace;
mod workspace_audit;
mod workspace_view;
mod write_journal;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version=build_version(), about = "Human-supervised step workflow harness")]
struct Cli {
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}
fn build_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| {
            format!(
                "{} ({})",
                env!("CARGO_PKG_VERSION"),
                option_env!("GREENLIGHT_BUILD_COMMIT").unwrap_or("source build")
            )
        })
        .as_str()
}
#[derive(Subcommand)]
enum Commands {
    /// Initialize project-local settings and bundled workflows without overwriting files
    Init,
    /// Validate a workflow and referenced step files
    Validate { workflow: String },
    /// Verify Linux shell sandbox requirements and isolation
    SandboxCheck,
    /// Validate real isolation with synthetic projects and emit a JSON benchmark report
    SandboxBenchmark {
        #[arg(long, default_value_t=30, value_parser=clap::value_parser!(u8).range(1..=100))]
        warm: u8,
        #[arg(long, default_value_t=5, value_parser=clap::value_parser!(u8).range(1..=20))]
        cold: u8,
        #[arg(long)]
        keep_failures: bool,
        /// Explicitly fetch reviewed public pins and provision Node/pre-commit offline
        #[arg(long)]
        public_dependencies: bool,
        /// Seed fixture caches from externally obtained pinned files; not a broker download
        #[arg(long, requires = "public_dependencies")]
        artifact_directory: Option<PathBuf>,
    },
    /// Run an explicitly selected named check against a disposable source copy
    Check { session: String, recipe: String },
    /// Fetch only the exact pinned public artifacts configured for a recipe
    CheckPrepare { session: String, recipe: String },
    /// Clear this project's prepared dependency objects/manifests, never source
    CheckDependenciesClear {
        session: String,
        #[arg(long)]
        yes: bool,
    },
    /// Inspect a branch-visible check's exact input manifest and generated diff
    CheckShow { session: String, run: String },
    /// Import reviewed check edits through the native journal (requires --yes)
    CheckImport {
        session: String,
        run: String,
        #[arg(long, required = true)]
        file: Vec<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Reconcile interrupted native writes without rewriting source files
    RecoverWrites,
    /// Recover a session whose execution owner died; never resumes work automatically
    RecoverSession { session: String },
    /// Start a new session, then execute
    Start {
        workflow: String,
        #[arg(long)]
        goal: String,
        #[arg(long)]
        model: Option<String>,
    },
    /// Continue a stopped session (user may add instructions)
    Resume {
        session: String,
        #[arg(long)]
        instruction: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    /// Open the live, read-only session viewer (optional session ID)
    Tui { session: Option<String> },
    /// Ask for an answer or proposal; this command never executes a route
    Ask {
        session: String,
        #[arg(long, required_unless_present = "file", conflicts_with = "file")]
        message: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        artifact: Vec<String>,
        /// Reply to this recorded manager clarification result
        #[arg(long)]
        reply_to: Option<i64>,
    },
    /// Generate a read-only, evidence-linked summary (one optional provider request)
    Summarize { session: String },
    /// Inspect the latest branch-visible summary without inference
    Summary { session: String },
    /// Accept a recorded manager forward; start only if it targets the current idle step
    DeliverRoute {
        session: String,
        decision_event: i64,
        #[arg(long)]
        queue_only: bool,
    },
    /// Preview and explicitly confirm a manager-proposed restart with its original instruction
    RestartRoute {
        session: String,
        message_id: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Explicitly continue a recorded success transition after undelivered guidance review
    ContinueGuidance { session: String },
    /// Inspect pending/durable guidance for the active branch (no inference)
    Routes { session: String },
    /// Withdraw pending guidance before request inclusion, including during execution
    WithdrawRoute { session: String, message_id: String },
    /// Preview the bounded project/pipeline evidence shared with the manager; no inference
    ManagerContext {
        session: String,
        #[arg(long)]
        artifact: Vec<String>,
    },
    /// Read manager exchanges separately from coding activity
    ManagerHistory { session: String },
    /// List saved sessions
    Sessions,
    /// Inspect recorded token counts and estimated costs (never assumes missing usage is zero)
    Usage {
        session: String,
        #[arg(long, value_enum, default_value = "session")]
        scope: usage::Scope,
        #[arg(long)]
        json: bool,
    },
    /// Show model metadata, discovering supported public provider rates when needed
    ModelInfo {
        model: Option<String>,
        #[arg(long)]
        refresh_pricing: bool,
    },
    /// Refresh supported public pricing catalog (no inference or credential transmission)
    RefreshPricing,
    /// List durable step attempts visible on the session's active branch
    Attempts { session: String },
    /// Compare current project files with the latest recorded step-boundary hashes
    WorkspaceStatus {
        session: String,
        #[arg(long)]
        json: bool,
    },
    /// Measure bounded read-only workspace scans without running a session
    WorkspaceBenchmark {
        #[arg(long, default_value_t=5, value_parser=clap::value_parser!(u8).range(1..=20))]
        runs: u8,
        #[arg(long)]
        json: bool,
    },
    /// Show event history, including current branch ancestry
    History { session: String },
    /// Export complete recorded conversation, protocol corrections and tool evidence
    Dump {
        session: String,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long, conflicts_with = "branch")]
        all_branches: bool,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// List session branches and their fork points
    Branches { session: String },
    /// List every historical run and its guarded restart checkpoint
    RestartPoints { session: String },
    /// Compare recorded branch workspace hashes without switching or restoring
    CompareBranches {
        session: String,
        #[arg(long)]
        left: String,
        #[arg(long)]
        right: String,
    },
    /// Select a historical branch (manual step choice; does NOT restore files)
    Switch {
        session: String,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        step: String,
    },
    /// List session artifacts on current branch
    Artifacts { session: String },
    /// Display a specific artifact
    Artifact {
        session: String,
        name: String,
        #[arg(
            long,
            help = "Export this exact creation event if visible on the active branch"
        )]
        event: Option<i64>,
    },
    /// Register a hand-edited artifact version for the current branch
    PutArtifact {
        session: String,
        name: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Preview and optionally restore the tracked workspace at a history event
    Restore {
        session: String,
        #[arg(long)]
        event: i64,
        #[arg(long)]
        dry_run: bool,
    },
    /// Fork from an event, offering to restore tracked files first
    Fork {
        session: String,
        #[arg(long)]
        event: i64,
        #[arg(long)]
        step: String,
        /// Skip workspace inspection/restoration; files remain unchanged
        #[arg(long)]
        no_restore: bool,
    },
    /// Show saved approvals and their file paths; optionally include one session
    Approvals {
        #[arg(long)]
        session: Option<String>,
    },
    /// Save historical verification evidence for the selected session/active branch
    VerificationEvidence { session: String },
    /// Show configured session model budgets, reservations and model roles
    ModelBudget { session: String },
    /// List native agent edits on the active branch
    Changes { session: String },
    /// Show a unified diff for one native agent write
    Diff {
        session: String,
        #[arg(long)]
        edit: i64,
    },
    /// List models through configured provider
    Models {
        #[arg(long)]
        pricing: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    let command = cli.command.unwrap_or(Commands::Tui { session: None });
    let root = project::resolve_root(cli.root.as_deref(), &cwd, matches!(command, Commands::Init))?;
    if !matches!(command, Commands::SandboxBenchmark { .. }) {
        project::load_environment(&root)?;
    }
    match command {
        Commands::Init => {
            println!("{}", project::init(&root)?);
        }
        Commands::Ask {
            session,
            message,
            file,
            artifact,
            reply_to,
        } => {
            let text = match file {
                Some(path) => manager::read_message_file(&path)?,
                None => message.context("missing message")?,
            };
            let engine = engine::Engine::open(&root)?;
            let reply = if reply_to.is_some() {
                let saved = manager::save_clarification(&engine.store, &session, &text, reply_to)?;
                engine.ask_saved_manager(&saved, &artifact).await?
            } else {
                engine.ask_manager(&session, &text, &artifact).await?
            };
            println!("{}", reply.display());
        }
        Commands::Summarize { session } => {
            let engine = engine::Engine::open(&root)?;
            println!("{}", engine.summarize(&session, None).await?.display());
        }
        Commands::Summary { session } => {
            let store = store::Store::open(&root)?;
            let summary = store
                .latest_summary(&store.session(&session)?)?
                .context("No recorded summary on this branch")?;
            println!("{}", summary.display());
        }
        Commands::DeliverRoute {
            session,
            decision_event,
            queue_only,
        } => {
            let mut engine = engine::Engine::open(&root)?;
            let accepted = engine
                .store
                .accept_manager_route(&root, &session, decision_event)?;
            let route = &accepted.route;
            println!("{}", route.display());
            if accepted.newly_accepted && accepted.start_current && !queue_only {
                engine
                    .run_manager_route(&session, &route.message_id)
                    .await?;
            }
        }
        Commands::RestartRoute {
            session,
            message_id,
            yes,
            dry_run,
        } => {
            let mut engine = engine::Engine::open(&root)?;
            let review = engine
                .store
                .prepare_manager_restart(&root, &session, &message_id)?;
            println!(
                "Restart target: {}\n{}",
                review.route.step,
                review.route.display()
            );
            review.plan.preview();
            if !dry_run
                && (yes || restore::confirm(
                    "Restore, fork and reserve this original instruction for the selected step?",
                )?)
            {
                let mut state = engine.store.session(&session)?;
                let route = engine
                    .store
                    .confirm_manager_restart(&root, &mut state, review, true)?;
                engine
                    .run_manager_route(&session, &route.message_id)
                    .await?;
            }
        }
        Commands::ContinueGuidance { session } => {
            let store = store::Store::open(&root)?;
            store.continue_after_guidance(&root, &session)?;
            println!("Guidance hold resolved. Recorded success transition applied; resume explicitly if another step is ready.");
        }
        Commands::Routes { session } => {
            let store = store::Store::open(&root)?;
            for route in store.manager_routes(&store.session(&session)?)? {
                println!("{}\n", route.display());
            }
        }
        Commands::WithdrawRoute {
            session,
            message_id,
        } => {
            let store = store::Store::open(&root)?;
            store.withdraw_manager_route(&session, &message_id)?;
            println!("Instruction withdrawn; original evidence retained.");
        }
        Commands::ManagerContext { session, artifact } => {
            let store = store::Store::open(&root)?;
            let snapshot = manager::capture(&root, &store, &session, "preview", &artifact)?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Commands::ManagerHistory { session } => {
            let store = store::Store::open(&root)?;
            for (id, step, kind, payload) in store
                .history(&store.session(&session)?)?
                .into_iter()
                .filter(|e| e.2.starts_with("manager."))
            {
                println!(
                    "{}",
                    serde_json::json!({"event":id,"step":step,"kind":kind,"payload":payload})
                );
            }
        }
        Commands::Usage {
            session,
            scope,
            json,
        } => {
            let store = store::Store::open(&root)?;
            let report = store.usage_report(&store.session(&session)?, scope)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.display());
            }
        }
        Commands::ModelInfo {
            model,
            refresh_pricing,
        } => {
            let cfg = config::load_config(&root)?;
            let model = model.as_deref().unwrap_or(&cfg.provider.model);
            let metadata = pricing::resolve(&root, &cfg.provider, model, refresh_pricing).await;
            println!(
                "Model: {model}\n{}",
                serde_json::to_string_pretty(&metadata)?
            );
            if let Some(p) = &metadata.pricing {
                println!("{}", p.freshness(chrono::Utc::now()));
            }
            println!("Null rates are unknown, never free. Prices are snapshots, not invoices. Tokenizer counts are estimates.");
        }
        Commands::RefreshPricing => {
            let cfg = config::load_config(&root)?;
            let catalog = pricing::catalog(&root, &cfg.provider, true).await?;
            if let Some(note) = catalog.note {
                bail!("{note} Cached model rates: {}", catalog.models.len());
            }
            println!("{} model rates cached. Public provider catalog retrieved; configured overrides still win.",catalog.models.len());
        }
        Commands::Attempts { session } => {
            let store = store::Store::open(&root)?;
            let selected = store.session(&session)?;
            for attempt in store.attempt_history(&selected)? {
                println!("{attempt}");
            }
        }
        Commands::RecoverSession { session } => {
            let store = store::Store::open(&root)?;
            println!("{}", store.recover_session(&session)?);
        }
        Commands::RecoverWrites => {
            let store = store::Store::open(&root)?;
            for outcome in store.recover_writes(&root)? {
                println!("{outcome}");
            }
            store.ensure_no_pending_writes()?;
            println!("Native write recovery complete; source files were not changed.");
        }
        Commands::Validate { workflow } => {
            config::load_config(&root)?;
            let wf = config::load_workflow(&root, &workflow)?;
            println!("Valid: {} ({} steps)", wf.definition.title, wf.steps.len());
        }
        Commands::Check { session, recipe } => {
            let mut engine = engine::Engine::open(&root)?;
            engine.run_named_check(&session, &recipe, false).await?;
        }
        Commands::CheckPrepare { session, recipe } => {
            let mut engine = engine::Engine::open(&root)?;
            engine.run_named_check(&session, &recipe, true).await?;
        }
        Commands::CheckDependenciesClear { session, yes } => {
            if !yes {
                bail!("This clears all prepared dependency caches for this project; pass --yes to confirm. Source is unchanged.");
            }
            let store = store::Store::open(&root)?;
            let session = store.session(&session)?;
            let owner = store.claim_tool_execution(&session.id, &session.active_branch)?;
            let result = check_dependencies::clear(&store, &session);
            store.finish_execution(&session.id, &owner, result.as_ref().err())?;
            println!("{}", result?);
        }
        Commands::CheckShow { session, run } => {
            let store = store::Store::open(&root)?;
            let session = store.session(&session)?;
            let receipt = verification_workspace::load(&store, &session, &run)?;
            println!(
                "{}\nRecorded command:\n{}\nFull raw diagnostic:\n{}\nPhase timings (milliseconds; absent = unmeasured/not reached):\n{}\nExecution measurements (observed output latency and Linux child accounting):\n{}\nConfigured offline preparation:\n{}\nVerified dependency inputs:\n{}\nOffline preparation output:\n{}\n{}\nManifest:\n{}\nExcluded inputs:\n{}",
                receipt.report(),
                receipt.command,
                receipt.diagnostic,
                serde_json::to_string_pretty(&receipt.phase_ms)?,
                serde_json::to_string_pretty(&receipt.executions)?,
                receipt.preparation_command.as_deref().unwrap_or("Not configured"),
                serde_json::to_string_pretty(&receipt.dependencies)?,
                receipt.preparation_diagnostic.as_deref().unwrap_or("Not configured/not run"),
                receipt.diff(),
                serde_json::to_string_pretty(&receipt.inputs)?,
                serde_json::to_string_pretty(&receipt.excluded)?
            );
        }
        Commands::CheckImport {
            session,
            run,
            file,
            yes,
        } => {
            if !yes {
                bail!("Review with check-show first; check-import requires --yes and explicit --file selections");
            }
            let store = store::Store::open(&root)?;
            let session = store.session(&session)?;
            let owner = store.claim_tool_execution(&session.id, &session.active_branch)?;
            let result = verification_workspace::import(&root, &store, &session, &run, &file);
            let cleanup = store.finish_execution(&session.id, &owner, result.as_ref().err());
            cleanup?;
            let message = result?;
            println!("{message}");
        }
        Commands::SandboxBenchmark {
            warm,
            cold,
            keep_failures,
            public_dependencies,
            artifact_directory,
        } => {
            let report = sandbox_validation::run(
                warm,
                cold,
                keep_failures,
                public_dependencies,
                artifact_directory.as_deref(),
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.gate_status != "passed" {
                bail!("Sandbox validation {}. Report written to stdout; no host fallback. GL-035 remains open.",report.gate_status);
            }
        }
        Commands::SandboxCheck => {
            let output = sandbox::Sandbox::open(&root)?
                .run(
                    "test ! -w /workspace && test -w /tmp && test -w /cache && \
                      test ! -e /workspace/.greenlight/state/history.db && \
                      printf 'Sandbox ready: project read-only, cache and tmp writable\\n'",
                )
                .await?;
            if !output.status.success() {
                bail!(
                    "sandbox health check failed (exit {:?}): {}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        Commands::Start {
            workflow,
            goal,
            model,
        } => {
            let mut engine = engine::Engine::open(&root)?;
            let wf = config::load_workflow(&root, &workflow)?;
            let session = engine
                .store
                .create(&workflow, &goal, &wf.definition.start)?;
            println!("Session ID: {}", session.id);
            engine.run(&session.id, None, model.as_deref()).await?;
        }
        Commands::Resume {
            session,
            instruction,
            model,
        } => {
            let mut engine = engine::Engine::open(&root)?;
            engine
                .run(&session, instruction.as_deref(), model.as_deref())
                .await?;
        }
        Commands::Tui { session } => {
            config::load_config(&root)?;
            tui::run(&root, session.as_deref())?;
        }
        Commands::WorkspaceStatus { session, json } => {
            let store = store::Store::open(&root)?;
            let selected = store.session(&session)?;
            let report = workspace_audit::status(&root, &store, &selected)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                if let Some(step) = &report.latest_step {
                    println!("Latest complete step recording:\n{}", step.display());
                }
                println!(
                    "Current workspace on branch {}:\n{}",
                    selected.active_branch,
                    report.current.display()
                );
            }
        }
        Commands::WorkspaceBenchmark { runs, json } => {
            let mut samples = Vec::new();
            for _ in 0..runs {
                let manifest = workspace::scan(&root)?;
                samples.push(serde_json::json!({"scan":manifest.scan_stats,"skipped":manifest.skipped.len(),"manifest_json_bytes":serde_json::to_vec(&manifest)?.len()}));
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&samples)?);
            } else {
                for (index, sample) in samples.iter().enumerate() {
                    println!("Scan {}: {} files, {} bytes hashed, {} µs, {} manifest JSON bytes, {} skipped",index+1,sample["scan"]["files"],sample["scan"]["bytes_hashed"],sample["scan"]["elapsed_us"],sample["manifest_json_bytes"],sample["skipped"]);
                }
                println!("Read-only scans; no contents saved. Cache state and disk speed affect timings; this does not measure full step/restore time.");
            }
        }
        Commands::Sessions => {
            let engine = engine::Engine::open(&root)?;
            for s in engine.store.list()? {
                println!(
                    "{}  {:<22} {:<12} {:<14} {}",
                    s.id, s.workflow, s.status, s.current_step, s.goal
                );
            }
        }
        Commands::Dump {
            session,
            branch,
            all_branches,
            json,
            output,
        } => {
            let store = store::Store::open(&root)?;
            let dump =
                conversation_dump::Dump::load(&store, &session, branch.as_deref(), all_branches)?;
            if let Some(path) = output {
                dump.save(&path, json)?;
                println!("Saved conversation dump: {}", path.display());
            } else if json {
                println!("{}", dump.json()?);
            } else {
                use std::io::Write;
                let stdout = std::io::stdout();
                let mut output = stdout.lock();
                // Escape directly to stdout; do not allocate another expanded copy.
                for c in dump.markdown()?.chars() {
                    if c.is_control() && c != '\n' && c != '\t' {
                        write!(output, "{}", c.escape_default())?;
                    } else {
                        write!(output, "{c}")?;
                    }
                }
                writeln!(output)?;
            }
        }
        Commands::History { session } => {
            let engine = engine::Engine::open(&root)?;
            let s = engine.store.session(&session)?;
            println!("branch: {}", s.active_branch);
            for (id, step, kind, data) in engine.store.history(&s)? {
                println!(
                    "#{id} [{step}] {kind}: {}",
                    data.replace('\n', " ")
                        .chars()
                        .take(180)
                        .collect::<String>()
                );
            }
        }
        Commands::RestartPoints { session } => {
            let store = store::Store::open(&root)?;
            let s = store.session(&session)?;
            let choices = tui_actions::candidates_for_session(&root, &store, &s)?;
            for (index, candidate) in choices.iter().enumerate() {
                println!(
                    "{} · step {} · --event {}",
                    tui_actions::restart_label(&choices, index),
                    candidate.step,
                    candidate.prior_event
                );
            }
        }
        Commands::CompareBranches {
            session,
            left,
            right,
        } => {
            let store = store::Store::open(&root)?;
            let s = store.session(&session)?;
            println!(
                "{}",
                tui_actions::compare_branches(&store, &s, &left, &right)?
            );
        }
        Commands::Branches { session } => {
            let engine = engine::Engine::open(&root)?;
            let s = engine.store.session(&session)?;
            for b in engine.store.branches(&session)? {
                let current = if b.id == s.active_branch { "*" } else { " " };
                println!(
                    "{current} {} (parent: {}, fork_event: {})",
                    b.id,
                    b.parent.as_deref().unwrap_or("none"),
                    b.fork_event
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "root".to_string())
                );
            }
        }
        Commands::Switch {
            session,
            branch,
            step,
        } => {
            let engine = engine::Engine::open(&root)?;
            let mut s = engine.store.session(&session)?;
            let wf = config::load_workflow(&root, &s.workflow)?;
            if !wf.steps.contains_key(&step) {
                bail!("unknown step {step}");
            }
            engine.store.switch_branch(&mut s, &branch, &step)?;
            println!("selected branch {branch} at {step}. Filesystem remains unchanged.");
        }
        Commands::Artifacts { session } => {
            let engine = engine::Engine::open(&root)?;
            let s = engine.store.session(&session)?;
            for a in engine.store.artifacts(&s, "session")? {
                println!("{}  ({}, {} bytes)", a.name, a.step, a.content.len());
            }
        }
        Commands::Artifact {
            session,
            name,
            event,
        } => {
            let engine = engine::Engine::open(&root)?;
            let session = engine.store.session(&session)?;
            let artifact = if let Some(event) = event {
                engine.store.artifact_version(&session, &name, event)?
            } else {
                engine
                    .store
                    .artifacts(&session, "session")?
                    .into_iter()
                    .find(|a| a.name == name)
                    .context("artifact not found")?
            };
            println!("{}", artifact.content);
        }
        Commands::PutArtifact {
            session,
            name,
            file,
        } => {
            let engine = engine::Engine::open(&root)?;
            let s = engine.store.session(&session)?;
            let content = std::fs::read_to_string(&file)?;
            if content.len() > engine.cfg.session.max_artifact_bytes {
                bail!("artifact exceeds configured size limit");
            }
            engine
                .store
                .with_project_control(&s, "artifact-edit", |lease| {
                    engine.store.check_project(lease, &s)?;
                    engine
                        .store
                        .add_artifact(&s, &s.current_step, &name, &content)?;
                    engine
                        .store
                        .event(&s, Some(&s.current_step), "user.artifact_edited", &name)?;
                    Ok(())
                })?;
            println!("Registered new artifact version: {name}");
        }
        Commands::Restore {
            session,
            event,
            dry_run,
        } => {
            let engine = engine::Engine::open(&root)?;
            let state = engine.store.session(&session)?;
            let plan = restore::RestorePlan::prepare(&root, &engine.store, &state, event)?;
            plan.preview();
            if !dry_run && !plan.is_empty() {
                if restore::confirm("Apply this restore?")? {
                    plan.apply(&root, &engine.store, &state)?;
                } else {
                    println!("Restore cancelled; no files changed.");
                }
            }
        }
        Commands::Fork {
            session,
            event,
            step,
            no_restore,
        } => {
            let engine = engine::Engine::open(&root)?;
            let mut state = engine.store.session(&session)?;
            let wf = config::load_workflow(&root, &state.workflow)?;
            if !wf.steps.contains_key(&step) {
                bail!("unknown step {step}");
            }
            let mut restoration = None;
            if !no_restore {
                let plan = restore::RestorePlan::prepare(&root, &engine.store, &state, event)?;
                plan.preview();
                if plan.is_empty()
                    || restore::confirm("Restore these tracked files before forking?")?
                {
                    restoration = Some(plan);
                } else {
                    println!(
                        "WARNING: forking without restoration; workspace may not match history."
                    );
                }
            } else {
                println!("WARNING: skipping workspace checks; fork does not restore files.");
            }
            if let Some(plan) = restoration {
                engine
                    .store
                    .restore_and_fork(&root, &mut state, plan, event, &step, true)?;
            } else {
                engine.store.fork(&mut state, event, &step)?;
            }
            println!("New branch {} at step {step}.", state.active_branch);
        }
        Commands::Approvals { session } => {
            let mut approvals = approvals::ApprovalStore::open(&root)?;
            if let Some(id) = session {
                let engine = engine::Engine::open(&root)?;
                let selected = engine.store.session(&id)?;
                approvals.bind_session(&selected.id)?;
            }
            print!("{}", approvals.describe()?);
        }
        Commands::ModelBudget { session } => {
            let config = config::load_config(&root)?;
            let store = store::Store::open(&root)?;
            let selected = store.session(&session)?;
            println!(
                "{}\nModel role defaults:\n{}",
                store.model_budget_status(&selected, &config.budgets)?,
                serde_yaml::to_string(&config.model_roles)?
            );
        }
        Commands::VerificationEvidence { session } => {
            let store = store::Store::open(&root)?;
            let selected = store.session(&session)?;
            println!("{}", store.save_verification_export(&selected)?.display());
        }
        Commands::Changes { session } => {
            let engine = engine::Engine::open(&root)?;
            let s = engine.store.session(&session)?;
            for change in engine.store.changes(&s)? {
                println!("#{}  [{}]  {}", change.event_id, change.step, change.path);
            }
        }
        Commands::Diff { session, edit } => {
            let engine = engine::Engine::open(&root)?;
            let s = engine.store.session(&session)?;
            let changes = engine.store.changes(&s)?;
            let change = changes
                .iter()
                .find(|c| c.event_id == edit)
                .context("edit not found in active branch history")?;
            print!(
                "{}",
                changes::unified(&change.path, change.before.as_deref(), &change.after)
            );
        }
        Commands::Models { pricing } => {
            let engine = engine::Engine::open(&root)?;
            for m in engine.provider.models().await? {
                println!("{m}");
                if pricing {
                    println!(
                        "{}",
                        serde_json::to_string(
                            &pricing::resolve(&root, &engine.cfg.provider, &m, false).await
                        )?
                    );
                }
            }
        }
    }
    Ok(())
}
