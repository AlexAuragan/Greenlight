mod approvals;
mod changes;
mod config;
mod engine;
mod markdown;
mod artifact_history;
mod activity_cards;
mod activity_titles;
mod paths;
mod provider;
mod provider_failure;
mod restore;
mod sandbox;
mod store;
mod tools;
mod tui;
mod tui_actions;
mod workspace;
mod workspace_audit;
mod write_journal;
mod session_execution;
mod input_pins;
mod locks;
mod usage;
mod pricing;
mod workspace_view;
mod session_picker;
mod manager;
mod project;
#[cfg(test)]
mod deviation_tests;

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
fn build_version()->&'static str {
    static VERSION:std::sync::OnceLock<String>=std::sync::OnceLock::new();
    VERSION.get_or_init(||format!("{} ({})",env!("CARGO_PKG_VERSION"),option_env!("GREENLIGHT_BUILD_COMMIT").unwrap_or("source build"))).as_str()
}
#[derive(Subcommand)]
enum Commands {
    /// Initialize project-local settings and bundled workflows without overwriting files
    Init,
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
    /// Ask the read-only manager; proposed forwarding never executes work in this stage
    Ask {session:String, #[arg(long, required_unless_present="file", conflicts_with="file")] message:Option<String>, #[arg(long)] file:Option<PathBuf>, #[arg(long)] artifact:Vec<String>},
    /// Preview the bounded project/pipeline evidence shared with the manager; no inference
    ManagerContext {session:String, #[arg(long)] artifact:Vec<String>},
    /// Read manager exchanges separately from coding activity
    ManagerHistory {session:String},
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
    /// Show model metadata, discovering supported public provider rates when needed
    ModelInfo { model: Option<String>, #[arg(long)] refresh_pricing: bool },
    /// Refresh supported public pricing catalog (no inference or credential transmission)
    RefreshPricing,
    /// List durable step attempts visible on the session's active branch
    Attempts { session: String },
    /// Compare current project files with the latest recorded step-boundary hashes
    WorkspaceStatus { session: String, #[arg(long)] json: bool },
    /// Measure bounded read-only workspace scans without running a session
    WorkspaceBenchmark { #[arg(long, default_value_t=5, value_parser=clap::value_parser!(u8).range(1..=20))] runs:u8, #[arg(long)] json:bool },
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
    Artifact { session: String, name: String, #[arg(long, help="Export this exact creation event if visible on the active branch")] event: Option<i64> },
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
    let cwd = std::env::current_dir()?;
    let command=cli.command.unwrap_or(Commands::Tui {session:None});
    let root=project::resolve_root(cli.root.as_deref(),&cwd,matches!(command,Commands::Init))?;
    project::load_environment(&root)?;
    match command {
        Commands::Init=>{println!("{}",project::init(&root)?);}
        Commands::Ask {session,message,file,artifact} => {
            let text=match file {Some(path)=>manager::read_message_file(&path)?,None=>message.context("missing message")?};
            let engine=engine::Engine::open(&root)?;
            let reply=engine.ask_manager(&session,&text,&artifact).await?;
            println!("{}",reply.display());
        }
        Commands::ManagerContext {session,artifact} => {
            let store=store::Store::open(&root)?;
            let snapshot=manager::capture(&root,&store,&session,"preview",&artifact)?;
            println!("{}",serde_json::to_string_pretty(&snapshot)?);
        }
        Commands::ManagerHistory {session} => {
            let store=store::Store::open(&root)?;
            for (id,step,kind,payload) in store.history(&store.session(&session)?)?.into_iter().filter(|e|e.2.starts_with("manager.")) {
                println!("{}",serde_json::json!({"event":id,"step":step,"kind":kind,"payload":payload}));
            }
        }
        Commands::Usage {session,scope,json} => {
            let store=store::Store::open(&root)?;
            let report=store.usage_report(&store.session(&session)?,scope)?;
            if json {println!("{}",serde_json::to_string_pretty(&report)?);} else {print!("{}",report.display());}
        }
        Commands::ModelInfo {model,refresh_pricing} => {
            let cfg=config::load_config(&root)?;
            let model=model.as_deref().unwrap_or(&cfg.provider.model);
            let metadata=pricing::resolve(&root,&cfg.provider,model,refresh_pricing).await;
            println!("Model: {model}\n{}",serde_json::to_string_pretty(&metadata)?);
            if let Some(p)=&metadata.pricing {println!("{}",p.freshness(chrono::Utc::now()));}
            println!("Null rates are unknown, never free. Prices are snapshots, not invoices. Tokenizer counts are estimates.");
        }
        Commands::RefreshPricing => {
            let cfg=config::load_config(&root)?;
            let catalog=pricing::catalog(&root,&cfg.provider,true).await?;
            if let Some(note)=catalog.note {bail!("{note} Cached model rates: {}",catalog.models.len());}
            println!("{} model rates cached. Public provider catalog retrieved; configured overrides still win.",catalog.models.len());
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
            config::load_config(&root)?;
            tui::run(&root, session.as_deref())?;
        }
        Commands::WorkspaceStatus { session,json } => {
            let store=store::Store::open(&root)?;
            let selected=store.session(&session)?;
            let report=workspace_audit::status(&root,&store,&selected)?;
            if json {println!("{}",serde_json::to_string_pretty(&report)?);} else {
                if let Some(step)=&report.latest_step {println!("Latest complete step recording:\n{}",step.display());}
                println!("Current workspace on branch {}:\n{}",selected.active_branch,report.current.display());
            }
        }
        Commands::WorkspaceBenchmark {runs,json} => {
            let mut samples=Vec::new();
            for _ in 0..runs {
                let manifest=workspace::scan(&root)?;
                samples.push(serde_json::json!({"scan":manifest.scan_stats,"skipped":manifest.skipped.len(),"manifest_json_bytes":serde_json::to_vec(&manifest)?.len()}));
            }
            if json {println!("{}",serde_json::to_string_pretty(&samples)?);} else {
                for (index,sample) in samples.iter().enumerate() {println!("Scan {}: {} files, {} bytes hashed, {} µs, {} manifest JSON bytes, {} skipped",index+1,sample["scan"]["files"],sample["scan"]["bytes_hashed"],sample["scan"]["elapsed_us"],sample["manifest_json_bytes"],sample["skipped"]);}
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
        Commands::Artifact { session, name, event } => {
            let engine=engine::Engine::open(&root)?;
            let session=engine.store.session(&session)?;
            let artifact=if let Some(event)=event {engine.store.artifact_version(&session,&name,event)?} else {engine.store.artifacts(&session,"session")?.into_iter().find(|a|a.name==name).context("artifact not found")?};
            println!("{}",artifact.content);
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
                if pricing {println!("{}",serde_json::to_string(&pricing::resolve(&root,&engine.cfg.provider,&m,false).await)?);}
            }
        }
    }
    Ok(())
}
