mod changes;
mod config;
mod engine;
mod provider;
mod store;
mod tools;

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
    /// List saved sessions
    Sessions,
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
    /// Create a new execution branch from an existing history event. Does NOT restore project files.
    Fork {
        session: String,
        #[arg(long)]
        event: i64,
        #[arg(long)]
        step: String,
    },
    /// List native agent edits on the active branch
    Changes { session: String },
    /// Show a unified diff for one native agent write
    Diff { session: String, #[arg(long)] edit: i64 },
    /// List models through configured provider
    Models,
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
        Commands::Validate { workflow } => {
            config::load_config(&root)?;
            let wf = config::load_workflow(&root, &workflow)?;
            println!("Valid: {} ({} steps)", wf.definition.title, wf.steps.len());
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
            let mut state = engine.store.session(&session)?;
            if state.status == "completed" {
                bail!("completed sessions require fork to run again");
            }
            if let Some(text) = instruction.as_deref() {
                engine
                    .store
                    .event(&state, Some(&state.current_step), "user.instruction", text)?;
            }
            state.status = "ready".into();
            engine.store.save(&state)?;
            engine
                .run(&session, instruction.as_deref(), model.as_deref())
                .await?;
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
        Commands::Fork {
            session,
            event,
            step,
        } => {
            let engine = engine::Engine::open(&root)?;
            let mut state = engine.store.session(&session)?;
            let wf = config::load_workflow(&root, &state.workflow)?;
            if !wf.steps.contains_key(&step) {
                bail!("unknown step {step}");
            }
            engine.store.fork(&mut state, event, &step)?;
            println!(
                "New branch {} at step {step}; filesystem not restored",
                state.active_branch
            );
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
        Commands::Models => {
            let engine = engine::Engine::open(&root)?;
            for m in engine.provider.models().await? {
                println!("{m}");
            }
        }
    }
    Ok(())
}
