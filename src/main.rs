mod approvals;
mod changes;
mod config;
mod engine;
mod markdown;
mod paths;
mod provider;
mod restore;
mod sandbox;
mod store;
mod tools;
mod tui;
mod tui_actions;
mod workspace;
mod write_journal;
mod session_execution;
mod input_pins;
mod locks;
mod usage;
mod workspace_view;
mod session_picker;
#[cfg(test)]
mod deviation_tests;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Human-supervised step workflow harness")]
struct Cli {
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Validate a workflow and referenced step files
    Validate { workflow: String },
    /// Verify Linux shell sandbox requirements and isolation
    SandboxCheck,
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
    /// List saved sessions
    Sessions,
    /// Inspect recorded token counts and estimated costs (never assumes missing usage is zero)
    Usage {
        session: String,
        #[arg(long,value_enum,default_value="session")]
        scope: usage::Scope,
        #[arg(long)]
        json: bool,
    },
    /// Show configured tokenizer/pricing metadata; does not query a provider
    ModelInfo { model: Option<String> },
    /// List durable step attempts visible on the session's active branch
    Attempts { session: String },
    /// Compare current project files with the latest recorded step-boundary hashes
    WorkspaceStatus { session: String },
    /// Show event history, including current branch ancestry
    History { session: String },
    /// List session branches and their fork points
    Branches { session: String },
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
    Artifact { session: String, name: String },
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
    /// Show the project/global persistent approvals and their file paths
    Approvals,
    /// List native agent edits on the active branch
    Changes { session: String },
    /// Show a unified diff for one native agent write
    Diff { session: String, #[arg(long)] edit: i64 },
    /// List models through configured provider
    Models { #[arg(long)] pricing: bool },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.root.canonicalize().context("invalid --root")?;
    // Populate missing environment variables from a `.env` file in the current
    // directory, then in the project root. Existing variables take precedence.
    let cwd = std::env::current_dir()?;
    config::load_dotenv(&cwd.join(".env"))?;
    config::load_dotenv(&root.join(".env"))?;
    match cli.command {
        Commands::Usage {session,scope,json} => {
            let store=store::Store::open(&root)?;
            let report=store.usage_report(&store.session(&session)?,scope)?;
            if json {println!("{}",serde_json::to_string_pretty(&report)?);} else {print!("{}",report.display());}
        }
        Commands::ModelInfo {model} => {
            let cfg=config::load_config(&root)?;
            let model=model.as_deref().unwrap_or(&cfg.provider.model);
            let metadata=cfg.provider.model_metadata.get(model).cloned().unwrap_or_default();
            println!("Model: {model}\n{}",serde_json::to_string_pretty(&metadata)?);
            println!("Null rates are unknown, never free. Configured pricing is not live-verified. Tokenizer counts are estimates.");
        }
        Commands::Attempts { session } => {
            let store = store::Store::open(&root)?;
            let selected = store.session(&session)?;
            for attempt in store.attempt_history(&selected)? { println!("{attempt}"); }
        }
        Commands::RecoverSession { session } => {
            let store = store::Store::open(&root)?;
            println!("{}", store.recover_session(&session)?);
        }
        Commands::RecoverWrites => {
            let store = store::Store::open(&root)?;
            for outcome in store.recover_writes(&root)? { println!("{outcome}"); }
            store.ensure_no_pending_writes()?;
            println!("Native write recovery complete; source files were not changed.");
        }
        Commands::Validate { workflow } => {
            config::load_config(&root)?;
            let wf = config::load_workflow(&root, &workflow)?;
            println!("Valid: {} ({} steps)", wf.definition.title, wf.steps.len());
        }
        Commands::SandboxCheck => {
            let output = sandbox::Sandbox::open(&root)?
                .run("test ! -w /workspace && test -w /tmp && test -w /cache && \
                      test ! -e /workspace/.greenlight/state/history.db && \
                      printf 'Sandbox ready: project read-only, cache and tmp writable\\n'")
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
            tui::run(&root, session.as_deref())?;
        }
        Commands::WorkspaceStatus { session } => {
            let engine = engine::Engine::open(&root)?;
            let selected = engine.store.session(&session)?;
            let Some((event, step, recorded)) = workspace::latest(&engine.store, &selected)? else {
                bail!("no workspace recording for session {session}; run at least one step");
            };
            let current = workspace::scan(&root)?;
            let changes = workspace::differences(&recorded, &current);
            if let Some((step_name, step_changes)) =
                workspace::latest_step_changes(&engine.store, &selected)?
            {
                println!("Latest finished step recording: {step_name}");
                if step_changes.is_empty() {
                    println!("  No scanned project-file changes during that step.");
                } else {
                    for change in step_changes {
                        println!("  {} during step: {}", change.change, change.path);
                    }
                }
            }
            println!("Current workspace vs recorded event #{event} (step {step}, branch {})", selected.active_branch);
            println!("Recorded: {} files; current: {} files", recorded.files.len(), current.files.len());
            println!("Skipped oversized/non-UTF8 paths: {} recorded; {} current", recorded.skipped.len(), current.skipped.len());
            if changes.is_empty() {
                println!("No changes since the most recent workspace recording.");
            } else {
                for change in changes {
                    println!("{} since recording: {}", change.change, change.path);
                }
            }
            println!("Hashes detect differences only; no file was modified or restored.");
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
        Commands::Artifact { session, name } => {
            let engine = engine::Engine::open(&root)?;
            let s = engine.store.session(&session)?;
            let artifacts = engine.store.artifacts(&s, "session")?;
            let item = artifacts
                .iter()
                .find(|a| a.name == name)
                .context("artifact not found")?;
            println!("{}", item.content);
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
                .add_artifact(&s, &s.current_step, &name, &content)?;
            engine
                .store
                .event(&s, Some(&s.current_step), "user.artifact_edited", &name)?;
            println!("Registered new artifact version: {name}");
        }
        Commands::Restore { session, event, dry_run } => {
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
        Commands::Fork { session, event, step, no_restore } => {
            let engine = engine::Engine::open(&root)?;
            let mut state = engine.store.session(&session)?;
            let wf = config::load_workflow(&root, &state.workflow)?;
            if !wf.steps.contains_key(&step) {
                bail!("unknown step {step}");
            }
            if !no_restore {
                let plan = restore::RestorePlan::prepare(&root, &engine.store, &state, event)?;
                plan.preview();
                if !plan.is_empty() {
                    if restore::confirm("Restore these tracked files before forking?")? {
                        plan.apply(&root, &engine.store, &state)?;
                    } else {
                        println!("WARNING: forking without restoration; workspace may not match history.");
                    }
                }
            } else {
                println!("WARNING: skipping workspace checks; fork does not restore files.");
            }
            engine.store.fork(&mut state, event, &step)?;
            println!("New branch {} at step {step}.", state.active_branch);
        }
        Commands::Approvals => {
            let approvals = approvals::ApprovalStore::open(&root)?;
            print!("{}", approvals.describe()?);
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
            let change = changes.iter().find(|c| c.event_id == edit)
                .context("edit not found in active branch history")?;
            print!("{}", changes::unified(&change.path, change.before.as_deref(), &change.after));
        }
        Commands::Models {pricing} => {
            let engine = engine::Engine::open(&root)?;
            for m in engine.provider.models().await? {
                println!("{m}");
                if pricing {println!("{}",serde_json::to_string(&engine.cfg.provider.model_metadata.get(&m).cloned().unwrap_or_default())?);}
            }
        }
    }
    Ok(())
}
