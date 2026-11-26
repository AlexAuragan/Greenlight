//! Offline checks operate on private source copies. Live imports use the native
//! journal and recheck every captured input; command approval never grants them.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const MAX_FILE: u64 = 4 * 1024 * 1024;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_EDITS: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub command: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub git: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparation: Option<crate::check_dependencies::Preparation>,
}
impl Recipe {
    pub fn validate(&self) -> Result<()> {
        if let Some(preparation) = &self.preparation {
            preparation.validate()?;
        }
        if self.command.trim().is_empty() || self.command.len() > 16_384 {
            bail!("check command must contain 1..16384 bytes");
        }
        if self.requires.len() > 32
            || self.requires.iter().any(|s| {
                s.is_empty()
                    || s.starts_with('-')
                    || s.len() > 128
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
            })
        {
            bail!(
                "check requires must be at most 32 executable basenames (no paths or shell syntax)"
            );
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> String {
        hash(serde_json::to_string(self).unwrap().as_bytes())
    }
    pub fn description(&self, name: &str) -> String {
        let preparation = self.preparation.as_ref().map(|p| format!("Offline bootstrap: {}\nPinned dependency artifacts: {} (fetched only by explicit preparation)\n",p.run.as_deref().unwrap_or("none"),p.artifacts.len())).unwrap_or_default();
        format!("{preparation}Named check: {name}\nCommand: {}\nProfile: disposable writable permitted-source snapshot; live project unchanged\nGit: {}\nNetwork/host services: unavailable; private HOME and job-local cache\nDependencies: only installed sandbox runtimes/offline artifacts; no downloads\nRequires: {}\nRecipe fingerprint: {}\n", self.command, if self.git {"fresh synthetic all-files index; no original index/history/config/hooks"} else {"no harness-provided metadata"}, self.requires.join(", "), self.fingerprint())
    }
}
// Diagnostic-only host lookup. Never run, mount or authorize these candidates.
// Absolute PATH directories only: relative entries would inspect project code.
fn runtime_hints(required: &[String], stderr: &str, host_path: Option<&std::ffi::OsStr>) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mut hints = String::new();
    for executable in required {
        let marker = format!("Required runtime unavailable: {executable}");
        if !stderr.lines().any(|line| line == marker) {
            continue;
        }
        let candidate = host_path.and_then(|path| {
            std::env::split_paths(path)
                .take(32)
                .filter(|directory| directory.is_absolute())
                .map(|directory| directory.join(executable))
                .find(|path| {
                    std::fs::metadata(path)
                        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                })
        });
        match candidate {
            Some(path) => hints.push_str(&format!("\n{executable}: host candidate {path:?} is not usable through the sandbox PATH. Host PATH and virtualenvs are not inherited; no candidate was executed or mounted.")),
            None => hints.push_str(&format!("\n{executable}: no executable found in the first 32 absolute host PATH directories. Install a supported system runtime or configure a separately prepared offline check.")),
        }
    }
    if !hints.is_empty() {
        hints.push_str("\nDeclare required interpreters/linkers in recipe.requires (for example cc for Rust tests). Presence checks do not prove all transitive libraries or services are available.");
    }
    hints
}

pub fn capabilities() -> &'static str {
    "Project-shell capabilities: /workspace is read-only; /tmp and /cache writable; network and host user services/sockets unavailable; original .git/config/hooks and secrets masked. Runtime/namespace readiness is not assumed. Native read remains project-scoped. Host virtualenvs and their absolute shebangs are not provisioned. Use an explicitly configured check for a disposable writable snapshot/private Git, or request manual user evidence. Shell approval/rerun does not change these capabilities."
}
pub(crate) fn is_generated_diff(name: &str) -> bool {
    name.strip_prefix("verification-")
        .and_then(|name| name.strip_suffix(".patch"))
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn allowed(path: &Path) -> bool {
    crate::file_discovery::path_in_scope(path)
        && !path
            .components()
            .any(|part| crate::sandbox::is_sensitive(&part.as_os_str().to_string_lossy()))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub hash: String,
    pub executable: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Edit {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub session: String,
    pub branch: String,
    pub step: String,
    pub recipe: String,
    pub fingerprint: String,
    pub command: String,
    pub git: bool,
    pub modifications: String,
    pub live_inputs_match: bool,
    pub outcome: String,
    pub launch: String,
    pub cleanup: String,
    /// Monotonic phase durations; absent keys mean unmeasured/not reached.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub phase_ms: BTreeMap<String, f64>,
    #[serde(default)]
    pub dependency_fingerprint: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<crate::check_dependencies::Artifact>,
    #[serde(default)]
    pub preparation_command: Option<String>,
    #[serde(default)]
    pub preparation_diagnostic: Option<String>,
    pub diagnostic: String,
    pub inputs: BTreeMap<String, Input>,
    pub excluded: Vec<String>,
    pub edits: Vec<Edit>,
    pub imported: BTreeSet<String>,
}
impl Receipt {
    pub fn report(&self) -> String {
        let imported = format!(
            "User-imported files: {} (fresh validation required)\n",
            self.imported.len()
        );
        let preparation = self
            .preparation_diagnostic
            .as_ref()
            .map(|s| {
                format!(
                    "Offline preparation output preview:\n{}\n",
                    s.chars().take(4000).collect::<String>()
                )
            })
            .unwrap_or_default();
        let dependencies = self
            .dependency_fingerprint
            .as_ref()
            .map(|f| {
                format!(
                    "Verified dependency inputs: {} artifacts · {f}\n",
                    self.dependencies.len()
                )
            })
            .unwrap_or_default();
        let state = format!(
            "Modifications: {} · live inputs still match: {}\n",
            self.modifications, self.live_inputs_match
        );
        format!("Check {} · {}\nRun: {}\n{state}{imported}Check command launched: {} · cleanup: {}\nGit: {}\nInputs: {} files · {} exclusions (manifest retained)\nRetained proposed source edits: {} · no automatic live writes\nRecipe fingerprint: {}\n{dependencies}{preparation}Diagnostic preview (full capture in check-show / recorded diagnostic):\n{}\nInspect this run with check-show (full raw diagnostic, manifest and diff); small diffs also appear as verification-{}.patch in Artifacts. Import is a separate user action; imported changes require fresh validation.\n", self.recipe, self.outcome, self.id, self.launch, self.cleanup, if self.git {"synthetic all-files; no original index/history"} else {"none provided by harness"}, self.inputs.len(), self.excluded.len(), self.edits.len(), self.fingerprint, self.diagnostic.chars().take(12_000).collect::<String>(), self.id)
    }
    pub fn diff(&self) -> String {
        let mut text = String::new();
        for edit in &self.edits {
            if let Some(after) = &edit.after {
                text.push_str(&crate::changes::unified(
                    &edit.path,
                    edit.before.as_deref(),
                    after,
                ));
            } else {
                text.push_str(&format!("\n{}: deleted/binary/unsupported change; manual review required, native import unavailable.\n", edit.path));
            }
        }
        if text.is_empty() {
            text.push_str("No permitted source edits recorded.\n");
        }
        text
    }
}

// Capture whole permitted inputs with its own larger budget. No symlinks,
// devices, silent over-budget omissions, original Git or dependency directories.
fn capture(
    source: &crate::anchored_fs::Dir,
    destination: Option<&Path>,
    stop: &AtomicBool,
) -> Result<(BTreeMap<String, Input>, Vec<String>)> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mut pending = vec![(PathBuf::new(), 0)];
    let mut inputs = BTreeMap::new();
    let mut excluded = Vec::new();
    let mut entries = 0;
    let mut path_bytes = 0;
    let mut bytes = 0;
    while let Some((path, depth)) = pending.pop() {
        if stop.load(Ordering::SeqCst) {
            bail!("Verification preparation cancelled; check not launched");
        }
        if depth > 32 {
            bail!("Verification snapshot exceeds depth 32");
        }
        let directory = source.open_dir(&path)?;
        for entry in directory.entries()? {
            if stop.load(Ordering::SeqCst) {
                bail!("Verification preparation cancelled; check not launched");
            }
            entries += 1;
            if entries > MAX_ENTRIES {
                bail!("Verification snapshot exceeds 20000 entries; nothing was silently omitted");
            }
            let entry = entry?;
            let relative = path.join(entry.name);
            let name = relative
                .to_str()
                .context("Verification requires UTF-8 source paths")?
                .to_owned();
            path_bytes += name.len();
            if path_bytes > 4 * 1024 * 1024 {
                bail!("Verification path manifest exceeds 4 MiB; narrow the project");
            }
            if !allowed(&relative) {
                excluded.push(name);
                continue;
            }
            if entry.directory {
                if let Some(dest) = destination {
                    std::fs::create_dir_all(dest.join(&relative))?;
                }
                pending.push((relative, depth + 1));
            } else if entry.regular {
                let mut file = source.open_file(&relative)?;
                let before = file.metadata()?;
                if before.len() > MAX_FILE {
                    bail!("Verification input {name} exceeds 4 MiB; no check launched");
                }
                let mut content = Vec::new();
                (&mut file).take(MAX_FILE + 1).read_to_end(&mut content)?;
                let after = file.metadata()?;
                if content.len() as u64 > MAX_FILE
                    || before.len() != after.len()
                    || before.ctime() != after.ctime()
                    || before.ctime_nsec() != after.ctime_nsec()
                    || before.mtime() != after.mtime()
                    || before.mtime_nsec() != after.mtime_nsec()
                    || after.len() != content.len() as u64
                {
                    bail!("Verification input {name} changed during capture; retry explicitly");
                }
                bytes += content.len() as u64;
                if bytes > MAX_BYTES {
                    bail!("Verification snapshot exceeds 64 MiB; no check launched");
                }
                let executable = before.mode() & 0o111 != 0;
                if let Some(dest) = destination {
                    let path = dest.join(&relative);
                    std::fs::create_dir_all(path.parent().unwrap())?;
                    let mut out = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?;
                    out.write_all(&content)?;
                    out.set_permissions(std::fs::Permissions::from_mode(if executable {
                        0o700
                    } else {
                        0o600
                    }))?;
                }
                inputs.insert(
                    name,
                    Input {
                        hash: hash(&content),
                        executable,
                    },
                );
            } else {
                bail!("Verification input {name} is a symlink/special file; provision a supported input instead");
            }
        }
    }
    excluded.sort();
    Ok((inputs, excluded))
}
fn same(a: &BTreeMap<String, Input>, b: &BTreeMap<String, Input>) -> bool {
    a.len() == b.len()
        && a.iter().all(|(path, input)| {
            b.get(path).is_some_and(|other| {
                other.hash == input.hash && other.executable == input.executable
            })
        })
}
fn run_directory(store: &crate::store::Store) -> Result<crate::anchored_fs::Dir> {
    store
        .directory_fd
        .create_dirs(Path::new("verification-runs"))
}
fn persist(store: &crate::store::Store, receipt: &Receipt) -> Result<()> {
    run_directory(store)?
        .target(Path::new(&format!("{}.json", receipt.id)))?
        .atomic_write(&serde_json::to_vec(receipt)?)
}

// Do not remove a writable copy or its cache while an invocation still owns
// them. Future cancellation drops this guard too; no async cleanup is assumed.
struct CheckCopy {
    directory: Option<tempfile::TempDir>,
    state: crate::anchored_fs::Dir,
}
impl CheckCopy {
    fn path(&self) -> &Path {
        self.directory.as_ref().unwrap().path()
    }
    fn retain(&mut self) {
        if let Some(directory) = self.directory.take() {
            let _ = directory.keep();
        }
    }
}
impl Drop for CheckCopy {
    fn drop(&mut self) {
        if crate::sandbox_job::ensure_clean(&self.state).is_err() {
            self.retain();
        }
    }
}

pub struct Request<'a> {
    pub root: &'a Path,
    pub store: &'a crate::store::Store,
    pub session: &'a crate::store::Session,
    pub step: &'a str,
    pub name: &'a str,
    pub recipe: &'a Recipe,
    pub stop: &'a Arc<AtomicBool>,
}
pub async fn run(request: Request<'_>, authorize: impl FnOnce() -> Result<()>) -> Result<Receipt> {
    let Request {
        root,
        store,
        session,
        step,
        name,
        recipe,
        stop,
    } = request;
    recipe.validate()?;
    let workspace_started = std::time::Instant::now();
    let id = uuid::Uuid::new_v4().to_string();
    let runs = run_directory(store)?;
    let mut view = CheckCopy {
        directory: Some(
            tempfile::Builder::new()
                .prefix(&format!("workspace-{id}-"))
                .tempdir_in(runs.path())?,
        ),
        state: store.directory_fd.open_dir(Path::new(""))?,
    };
    let source = crate::anchored_fs::Dir::open(&crate::sandbox::checked_project_root(root)?)?;
    store.event(
        session,
        Some(step),
        "verification.preparing",
        &recipe.description(name),
    )?;
    let holder = runs.open_dir(Path::new(view.path().file_name().unwrap()))?;
    let copy = holder.create_dirs(Path::new("source"))?;
    let cache = holder.create_dirs(Path::new("cache"))?;
    let dependencies = holder.create_dirs(Path::new("dependencies"))?;
    let sandbox = crate::sandbox::Sandbox::verification(&copy, &cache, root, recipe.git)?
        .with_dependencies(&dependencies)?;
    let mut receipt = Receipt {
        id,
        session: session.id.clone(),
        branch: session.active_branch.clone(),
        step: step.into(),
        recipe: name.into(),
        fingerprint: recipe.fingerprint(),
        command: recipe.command.clone(),
        git: recipe.git,
        modifications: "none".into(),
        live_inputs_match: false,
        outcome: "unavailable".into(),
        launch: "not_started".into(),
        cleanup: "confirmed".into(),
        phase_ms: BTreeMap::new(),
        dependency_fingerprint: None,
        dependencies: Vec::new(),
        preparation_command: recipe.preparation.as_ref().and_then(|p| p.run.clone()),
        preparation_diagnostic: None,
        diagnostic: String::new(),
        inputs: BTreeMap::new(),
        excluded: Vec::new(),
        edits: Vec::new(),
        imported: BTreeSet::new(),
    };
    receipt.phase_ms.insert(
        "workspace_setup".into(),
        workspace_started.elapsed().as_secs_f64() * 1000.0,
    );
    let mut required = recipe.requires.clone();
    if recipe.git && !required.iter().any(|name| name == "git") {
        required.push("git".into());
    }
    let mut probe = String::from("test -w /workspace && test -w /cache || exit 125; missing=0; ");
    for executable in &required {
        probe.push_str(&format!("command -v '{executable}' >/dev/null || {{ printf 'Required runtime unavailable: {executable}\\n' >&2; missing=1; }}; "));
    }
    probe.push_str("test \"$missing\" -eq 0 || exit 127; printf 'Sandbox profile ready\\n'");
    let started = std::time::Instant::now();
    let preflight = sandbox.run_stoppable(&probe, stop).await;
    receipt
        .phase_ms
        .insert("preflight".into(), started.elapsed().as_secs_f64() * 1000.0);
    match preflight {
        Ok(output) if output.status.success() => {}
        result => {
            receipt.diagnostic = match result {
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let hints =
                        runtime_hints(&required, &stderr, std::env::var_os("PATH").as_deref());
                    format!(
                        "Preflight refused; check not started. {}{stderr}{hints}",
                        String::from_utf8_lossy(&output.stdout)
                    )
                }
                Err(error) => {
                    if error.is::<crate::sandbox_job::CleanupIncomplete>() {
                        view.retain();
                        return Err(error);
                    }
                    format!("Preflight unavailable; check command not started: {error:#}")
                }
            };
            persist(store, &receipt)?;
            return Ok(receipt);
        }
    }
    let started = std::time::Instant::now();
    let staging =
        crate::check_dependencies::stage(&store.directory_fd, recipe, &dependencies, stop);
    receipt.phase_ms.insert(
        "dependency_staging".into(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    if let Err(error) = staging {
        receipt.diagnostic =
            format!("Dependency preparation unavailable; check command not started: {error:#}");
        persist(store, &receipt)?;
        return Ok(receipt);
    }
    if let Some(preparation) = recipe
        .preparation
        .as_ref()
        .filter(|p| !p.artifacts.is_empty())
    {
        receipt.dependency_fingerprint = Some(recipe.fingerprint());
        receipt.dependencies = preparation.artifacts.clone();
        store.event(
            session,
            Some(step),
            "verification.dependencies",
            &serde_json::to_string(&receipt.dependencies)?,
        )?;
    }
    let started = std::time::Instant::now();
    authorize()?;
    receipt.phase_ms.insert(
        "approval_wait".into(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    store.event(
        session,
        Some(step),
        "verification.preparing",
        "Sandbox requirements available; capturing permitted source inputs",
    )?;
    let started = std::time::Instant::now();
    let (inputs, excluded) = capture(&source, Some(&view.path().join("source")), stop)?;
    let baseline = capture(&source, None, stop)?.0;
    if !same(&inputs, &baseline) {
        bail!("Verification inputs changed during preparation; check not launched");
    }
    receipt.phase_ms.insert(
        "source_capture_and_recheck".into(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    receipt.inputs = inputs;
    receipt.excluded = excluded;
    if recipe.git {
        let started = std::time::Instant::now();
        let setup = sandbox.run_stoppable("git -c init.templateDir= -c core.hooksPath=/dev/null init -q && git -c core.hooksPath=/dev/null add -f -- .", stop).await;
        receipt.phase_ms.insert(
            "synthetic_git".into(),
            started.elapsed().as_secs_f64() * 1000.0,
        );
        match setup {
            Ok(output) if output.status.success() => {}
            result => {
                receipt.diagnostic = match result {
                    Ok(output) => format!(
                        "Synthetic Git preparation unavailable; check not started. {}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ),
                    Err(error) => {
                        if error.is::<crate::sandbox_job::CleanupIncomplete>() {
                            view.retain();
                            return Err(error);
                        }
                        format!(
                            "Synthetic Git preparation unavailable; check not started: {error:#}"
                        )
                    }
                };
                persist(store, &receipt)?;
                return Ok(receipt);
            }
        }
    }
    if let Some(command) = recipe.preparation.as_ref().and_then(|p| p.run.as_deref()) {
        store.event(session, Some(step), "verification.bootstrap", &format!("Offline dependency installation; no host execution/network.\nRecipe fingerprint: {}\n{command}", recipe.fingerprint()))?;
        let started = std::time::Instant::now();
        let output = sandbox.run_stoppable(command, stop).await;
        receipt.phase_ms.insert(
            "offline_setup".into(),
            started.elapsed().as_secs_f64() * 1000.0,
        );
        match output {
            Ok(output) => {
                receipt.preparation_diagnostic = Some(format!(
                    "Raw status: {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
                if !output.status.success() {
                    receipt.diagnostic = "Offline dependency preparation failed; check command not started. Inspect preparation output, not a claimed source regression.".into();
                    let started = std::time::Instant::now();
                    let inspection = record_changes(&mut receipt, &source, &copy);
                    receipt.phase_ms.insert(
                        "change_inspection".into(),
                        started.elapsed().as_secs_f64() * 1000.0,
                    );
                    inspection?;
                    persist(store, &receipt)?;
                    return Ok(receipt);
                }
            }
            Err(error) => {
                if error.is::<crate::sandbox_job::CleanupIncomplete>() {
                    view.retain();
                    return Err(error);
                }
                receipt.diagnostic = format!("Offline dependency preparation incomplete; check command not started: {error:#}");
                let started = std::time::Instant::now();
                let inspection = record_changes(&mut receipt, &source, &copy);
                receipt.phase_ms.insert(
                    "change_inspection".into(),
                    started.elapsed().as_secs_f64() * 1000.0,
                );
                inspection?;
                persist(store, &receipt)?;
                return Ok(receipt);
            }
        }
    }
    store.event(
        session,
        Some(step),
        "verification.running",
        &format!(
            "{}\nSnapshot {} · {} permitted files",
            recipe.description(name),
            receipt.id,
            receipt.inputs.len()
        ),
    )?;
    receipt.launch = "unknown".into();
    let started = std::time::Instant::now();
    let execution = sandbox.run_stoppable(&recipe.command, stop).await;
    receipt.phase_ms.insert(
        "check_execution".into(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    match execution {
        Ok(output) => {
            receipt.launch = "started".into();
            receipt.outcome = if output.status.success() {
                "passed"
            } else {
                "failed"
            }
            .into();
            receipt.diagnostic = format!(
                "Raw status: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Err(error) => {
            if error.is::<crate::sandbox_job::CleanupIncomplete>() {
                view.retain();
                return Err(error);
            }
            receipt.outcome = if stop.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "unknown"
            }
            .into();
            receipt.diagnostic =
                format!("Execution/capture failed; do not infer a source regression: {error:#}");
        }
    }
    let started = std::time::Instant::now();
    let inspection = record_changes(&mut receipt, &source, &copy);
    receipt.phase_ms.insert(
        "change_inspection".into(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    inspection?;
    persist(store, &receipt)?;
    Ok(receipt)
}

fn record_changes(
    receipt: &mut Receipt,
    source: &crate::anchored_fs::Dir,
    copy: &crate::anchored_fs::Dir,
) -> Result<()> {
    let after = match capture(copy, None, &AtomicBool::new(false)) {
        Ok((files, _)) => files,
        Err(error) => {
            receipt.outcome = "unknown".into();
            receipt.modifications = "unknown".into();
            receipt.diagnostic.push_str(&format!(
                "\nPost-check source inspection unavailable: {error:#}. No import offered."
            ));
            return Ok(());
        }
    };
    receipt.live_inputs_match = capture(source, None, &AtomicBool::new(false))
        .map(|(current, _)| same(&receipt.inputs, &current))
        .unwrap_or(false);
    let paths = receipt
        .inputs
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut budget = 0;
    for path in paths {
        if receipt.inputs.get(&path).map(|i| (&i.hash, i.executable))
            == after.get(&path).map(|i| (&i.hash, i.executable))
        {
            continue;
        }
        receipt.modifications = "present".into();
        let before = if receipt.inputs.contains_key(&path) {
            source.read_string(Path::new(&path), MAX_FILE).ok()
        } else {
            None
        };
        // Import preimages must come from the original snapshot, not a live file
        // edited while the check ran. If live inputs drifted, offer no import.
        if before.as_ref().is_some_and(|text| {
            receipt
                .inputs
                .get(&path)
                .is_none_or(|i| i.hash != hash(text.as_bytes()))
        }) {
            receipt
                .diagnostic
                .push_str("\nLive source drifted while check ran; generated imports withheld.");
            receipt.edits.clear();
            receipt.modifications = "unknown".into();
            break;
        }
        let mode_changed = receipt.inputs.get(&path).is_some_and(|i| {
            after
                .get(&path)
                .is_some_and(|a| i.executable != a.executable)
        }) || !receipt.inputs.contains_key(&path)
            && after.get(&path).is_some_and(|a| a.executable);
        let binary_before = receipt.inputs.contains_key(&path) && before.is_none();
        let content = if after.contains_key(&path) && !mode_changed && !binary_before {
            copy.read_string(Path::new(&path), MAX_FILE).ok()
        } else {
            None
        };
        budget += before.as_ref().map_or(0, String::len) + content.as_ref().map_or(0, String::len);
        if budget > MAX_EDITS {
            receipt.edits.clear();
            receipt.modifications = "unknown".into();
            receipt.modifications = "unknown".into();
            receipt
                .diagnostic
                .push_str("\nGenerated edits exceed 4 MiB retention budget; import unavailable.");
            break;
        }
        receipt.edits.push(Edit {
            path,
            before,
            after: content,
        });
    }
    Ok(())
}

pub fn load(
    store: &crate::store::Store,
    session: &crate::store::Session,
    id: &str,
) -> Result<Receipt> {
    uuid::Uuid::parse_str(id).context("Invalid verification run ID")?;
    let receipt: Receipt = serde_json::from_str(
        &run_directory(store)?
            .read_string(Path::new(&format!("{id}.json")), (64 * 1024 * 1024) as u64)?,
    )?;
    if receipt.session != session.id || receipt.branch != session.active_branch {
        bail!("Verification run belongs to another session/branch");
    }
    if !store
        .history(session)?
        .iter()
        .any(|e| e.2 == "verification.workspace" && e.3.contains(&format!("Run: {id}\n")))
    {
        bail!("Verification run is not published on this branch");
    }
    Ok(receipt)
}
pub fn import(
    root: &Path,
    store: &crate::store::Store,
    session: &crate::store::Session,
    id: &str,
    paths: &[String],
) -> Result<String> {
    let lease = store
        .active_project
        .borrow()
        .upgrade()
        .context("Verification import requires an owned execution")?;
    store.check_project(&lease, session)?;
    let mut receipt = load(store, session, id)?;
    if receipt.modifications != "present" {
        bail!("No complete generated edit set available for import");
    }
    if receipt.cleanup != "confirmed" || receipt.launch != "started" || paths.is_empty() {
        bail!("No confirmed check edits selected");
    }
    // Prevalidate the whole selection before applying its first file. Native
    // writes still recheck preimages independently; this is not atomic rollback.
    let directory = crate::anchored_fs::Dir::open(root)?;
    if !receipt.live_inputs_match {
        bail!("Check did not match live inputs at completion; run a fresh check");
    }
    let mut expected = receipt.inputs.clone();
    for path in &receipt.imported {
        let edit = receipt
            .edits
            .iter()
            .find(|e| &e.path == path)
            .context("Missing imported edit")?;
        let after = edit.after.as_ref().context("Deletion import unsupported")?;
        expected.insert(
            path.clone(),
            Input {
                hash: hash(after.as_bytes()),
                executable: receipt.inputs.get(path).is_some_and(|i| i.executable),
            },
        );
    }
    let current = capture(&directory, None, &AtomicBool::new(false))?.0;
    if !same(&expected, &current) {
        bail!("Live input drift since this check; import refused. Run a fresh check.");
    }
    if paths.iter().collect::<BTreeSet<_>>().len() != paths.len() {
        bail!("Duplicate import paths");
    }
    for path in paths {
        let edit = receipt
            .edits
            .iter()
            .find(|e| &e.path == path && !receipt.imported.contains(path))
            .context("File is not a pending generated edit")?;
        if !allowed(Path::new(path)) || edit.after.is_none() {
            bail!("Only allowed UTF-8 source writes can be imported; deletions/binary/mode changes require manual review");
        }
    }
    for path in paths {
        let edit = receipt.edits.iter().find(|e| &e.path == path).unwrap();
        store.apply_native_write(
            root,
            session,
            &receipt.step,
            path,
            edit.before.as_deref(),
            edit.after.as_deref().unwrap(),
        )?;
        receipt.imported.insert(path.clone());
        persist(store, &receipt)?;
        store.event(
            session,
            Some(&receipt.step),
            "verification.imported",
            &format!("Run: {id}\nFile: {path}\nInitiator: user\nNative journal write; requires fresh validation."),
        )?;
    }
    Ok(format!("Imported {} source file(s) through native journal. Re-run checks; this is not fresh verification.", paths.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn fixture() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        crate::store::Store,
        crate::store::Session,
        Receipt,
    ) {
        let root = tempfile::tempdir().unwrap();
        let copy = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.txt"), "before\n").unwrap();
        fs::write(root.path().join("b.txt"), "other\n").unwrap();
        let store = crate::store::Store::open(root.path()).unwrap();
        let session = store.create("demo", "fixture", "verify").unwrap();
        let source = crate::anchored_fs::Dir::open(root.path()).unwrap();
        let (inputs, excluded) =
            capture(&source, Some(copy.path()), &AtomicBool::new(false)).unwrap();
        let recipe = Recipe {
            command: "fixture-only".into(),
            requires: vec![],
            git: false,
            preparation: None,
        };
        let receipt = Receipt {
            id: uuid::Uuid::new_v4().to_string(),
            session: session.id.clone(),
            branch: session.active_branch.clone(),
            step: "verify".into(),
            recipe: "fixture".into(),
            fingerprint: recipe.fingerprint(),
            command: recipe.command,
            git: false,
            modifications: "none".into(),
            live_inputs_match: true,
            outcome: "passed".into(),
            launch: "started".into(),
            cleanup: "confirmed".into(),
            phase_ms: BTreeMap::new(),
            dependency_fingerprint: None,
            dependencies: Vec::new(),
            preparation_command: None,
            preparation_diagnostic: None,
            diagnostic: "Fixture evidence; no real check executed".into(),
            inputs,
            excluded,
            edits: vec![],
            imported: BTreeSet::new(),
        };
        (root, copy, store, session, receipt)
    }
    fn inspect(root: &Path, copy: &Path, receipt: &mut Receipt) {
        record_changes(
            receipt,
            &crate::anchored_fs::Dir::open(root).unwrap(),
            &crate::anchored_fs::Dir::open(copy).unwrap(),
        )
        .unwrap();
    }
    fn publish(store: &crate::store::Store, session: &crate::store::Session, receipt: &Receipt) {
        persist(store, receipt).unwrap();
        store
            .event(
                session,
                Some("verify"),
                "verification.workspace",
                &receipt.report(),
            )
            .unwrap();
    }
    #[test]
    fn legacy_recipe_fingerprint_and_receipts_remain_compatible() {
        let recipe: Recipe =
            serde_json::from_str(r#"{"command":"true","requires":[],"git":false}"#).unwrap();
        assert!(recipe.preparation.is_none());
        assert_eq!(
            recipe.fingerprint(),
            hash(br#"{"command":"true","requires":[],"git":false}"#)
        );
        let (_root, _copy, _store, _session, mut receipt) = fixture();
        receipt.preparation_diagnostic = Some("Recipe fingerprint: forged-from-output".into());
        assert_eq!(
            receipt
                .report()
                .lines()
                .find_map(|line| line.strip_prefix("Recipe fingerprint: ")),
            Some(receipt.fingerprint.as_str())
        );
        receipt.phase_ms.insert("check_execution".into(), 12.5);
        let mut value = serde_json::to_value(&receipt).unwrap();
        assert_eq!(
            serde_json::from_value::<Receipt>(value.clone())
                .unwrap()
                .phase_ms["check_execution"],
            12.5
        );
        value.as_object_mut().unwrap().remove("phase_ms");
        value
            .as_object_mut()
            .unwrap()
            .remove("dependency_fingerprint");
        value
            .as_object_mut()
            .unwrap()
            .remove("preparation_diagnostic");
        let old: Receipt = serde_json::from_value(value).unwrap();
        assert!(old.dependency_fingerprint.is_none() && old.preparation_diagnostic.is_none());
        assert!(old.phase_ms.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires working Bubblewrap user namespaces and Python venv support"]
    async fn integration_dependencies_are_readonly_and_venv_is_fresh_for_every_check() {
        let (root, _copy, store, session, _receipt) = fixture();
        let artifact = crate::check_dependencies::Artifact {
            name: "pinned.bin".into(),
            url: "https://artifacts.example.com/pinned.bin".into(),
            sha256: hash(b"pinned"),
        };
        let objects = store
            .directory_fd
            .create_dirs(Path::new("check-dependencies/objects"))
            .unwrap();
        objects
            .target(Path::new(&format!("{}.bin", artifact.sha256)))
            .unwrap()
            .atomic_write(b"pinned")
            .unwrap();
        let recipe = Recipe { command:r#"/cache/venv/bin/python -c 'from pathlib import Path; assert Path("/cache/copied").read_bytes() == b"pinned"'"#.into(),requires:vec!["python3".into()],git:false,
            preparation:Some(crate::check_dependencies::Preparation { artifacts:vec![artifact],run:Some("test ! -e /cache/venv && python3 -m venv --without-pip /cache/venv && cp /deps/pinned.bin /cache/copied && if printf poison > /deps/pinned.bin; then exit 91; fi".into()) }) };
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let request = || Request {
            root: root.path(),
            store: &store,
            session: &session,
            step: "verify",
            name: "tests",
            recipe: &recipe,
            stop: &stop,
        };
        crate::check_dependencies::prepare(request(), || Ok(()))
            .await
            .unwrap();
        for _ in 0..2 {
            let receipt = run(request(), || Ok(())).await.unwrap();
            assert_eq!(
                receipt.outcome, "passed",
                "{}\n{:?}",
                receipt.diagnostic, receipt.preparation_diagnostic
            );
            assert_eq!(receipt.modifications, "none");
            assert!(receipt.live_inputs_match);
            assert!(receipt
                .preparation_diagnostic
                .as_ref()
                .unwrap()
                .contains("status: 0"));
        }
        store.finish_execution(&session.id, &owner, None).unwrap();
    }

    #[test]
    fn edit_retention_overflow_is_unknown_and_cannot_be_imported_as_an_empty_clean_result() {
        let (root, copy, store, session, mut receipt) = fixture();
        let before = "a".repeat(MAX_EDITS / 4 + 1);
        let after = "b".repeat(MAX_EDITS / 4 + 1);
        for name in ["a.txt", "b.txt"] {
            fs::write(root.path().join(name), &before).unwrap();
            fs::write(copy.path().join(name), &after).unwrap();
        }
        receipt.inputs = capture(
            &crate::anchored_fs::Dir::open(root.path()).unwrap(),
            None,
            &AtomicBool::new(false),
        )
        .unwrap()
        .0;
        inspect(root.path(), copy.path(), &mut receipt);
        assert_eq!(receipt.modifications, "unknown");
        assert!(receipt.edits.is_empty());
        publish(&store, &session, &receipt);
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        assert!(import(
            root.path(),
            &store,
            &session,
            &receipt.id,
            &["a.txt".into()]
        )
        .is_err());
        store.finish_execution(&session.id, &owner, None).unwrap();
    }

    #[test]
    fn dropping_a_copy_with_unconfirmed_cleanup_retains_source_and_cache() {
        use std::os::unix::fs::FileExt;
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        fs::create_dir(workspace.path().join("source")).unwrap();
        fs::create_dir(workspace.path().join("cache")).unwrap();
        let path = workspace.path().to_path_buf();
        let state = crate::anchored_fs::Dir::open(root.path()).unwrap();
        let jobs = state.create_dirs(Path::new("sandbox-jobs")).unwrap();
        let pending = jobs.lock_file(Path::new("pending.lease")).unwrap();
        pending.write_all_at(b"pending ", 0).unwrap();
        drop(CheckCopy {
            directory: Some(workspace),
            state,
        });
        assert!(path.join("source").is_dir() && path.join("cache").is_dir());
        fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    #[ignore = "requires installed Bubblewrap and enabled Linux user namespaces"]
    async fn integration_synthetic_git_and_formatter_cannot_change_live_source() {
        let (root, _, store, session, _) = fixture();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(
            root.path().join(".git/config"),
            "[remote \"origin\"]\n url = secret-original-remote\n",
        )
        .unwrap();
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        let recipe = Recipe { command:"test -z \"$(git config --get remote.origin.url)\" && git ls-files --error-unmatch a.txt && printf formatted > a.txt".into(), requires:vec!["git".into()], git:true, preparation:None };
        let stop = Arc::new(AtomicBool::new(false));
        let receipt = run(
            Request {
                root: root.path(),
                store: &store,
                session: &session,
                step: "verify",
                name: "git-format",
                recipe: &recipe,
                stop: &stop,
            },
            || Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(receipt.outcome, "passed", "{}", receipt.diagnostic);
        assert_eq!(receipt.modifications, "present");
        assert_eq!(receipt.edits.len(), 1);
        assert_eq!(
            fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "before\n"
        );
        store.finish_execution(&session.id, &owner, None).unwrap();
    }

    #[test]
    fn snapshots_filter_private_generated_paths_and_reject_symlinks_or_oversize() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        fs::write(root.path().join("script"), "#!/bin/sh\n").unwrap();
        fs::set_permissions(
            root.path().join("script"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        for name in [".git", ".greenlight", "node_modules"] {
            fs::create_dir(root.path().join(name)).unwrap();
            fs::write(root.path().join(name).join("secret"), "hidden").unwrap();
        }
        fs::write(root.path().join(".env"), "hidden").unwrap();
        let source = crate::anchored_fs::Dir::open(root.path()).unwrap();
        let (manifest, excluded) =
            capture(&source, Some(dest.path()), &AtomicBool::new(false)).unwrap();
        assert_eq!(manifest.len(), 1);
        assert!(manifest["script"].executable);
        assert_eq!(excluded.len(), 4);
        assert!(!dest.path().join(".git").exists());
        assert!(
            fs::metadata(dest.path().join("script"))
                .unwrap()
                .permissions()
                .mode()
                & 0o100
                != 0
        );
        symlink("/etc/passwd", root.path().join("alias")).unwrap();
        assert!(capture(&source, None, &AtomicBool::new(false))
            .unwrap_err()
            .to_string()
            .contains("symlink"));
        fs::remove_file(root.path().join("alias")).unwrap();
        fs::File::create(root.path().join("large"))
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        assert!(capture(&source, None, &AtomicBool::new(false)).is_err());
        assert!(capture(&source, None, &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn changes_stay_in_copy_until_explicit_guarded_journal_import_and_support_selected_files() {
        let (root, copy, store, session, mut receipt) = fixture();
        fs::write(copy.path().join("a.txt"), "after\n").unwrap();
        fs::write(copy.path().join("new.txt"), "new\n").unwrap();
        inspect(root.path(), copy.path(), &mut receipt);
        assert_eq!(receipt.outcome, "passed");
        assert_eq!(receipt.modifications, "present");
        assert_eq!(
            fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "before\n"
        );
        assert!(receipt.diff().contains("-before\n+after\n"));
        publish(&store, &session, &receipt);
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        import(
            root.path(),
            &store,
            &session,
            &receipt.id,
            &["a.txt".into()],
        )
        .unwrap();
        assert!(!root.path().join("new.txt").exists());
        import(
            root.path(),
            &store,
            &session,
            &receipt.id,
            &["new.txt".into()],
        )
        .unwrap();
        assert_eq!(
            load(&store, &session, &receipt.id).unwrap().imported.len(),
            2
        );
        assert_eq!(
            fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "after\n"
        );
        assert!(store
            .history(&session)
            .unwrap()
            .iter()
            .any(|e| e.2 == "file.changed"));
        store.finish_execution(&session.id, &owner, None).unwrap();
    }
    #[test]
    fn import_rejects_unrelated_input_drift_duplicates_and_branch_changes() {
        let (root, copy, store, session, mut receipt) = fixture();
        fs::write(copy.path().join("a.txt"), "after\n").unwrap();
        inspect(root.path(), copy.path(), &mut receipt);
        publish(&store, &session, &receipt);
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        assert!(import(
            root.path(),
            &store,
            &session,
            &receipt.id,
            &["a.txt".into(), "a.txt".into()]
        )
        .is_err());
        fs::write(root.path().join("b.txt"), "manual drift").unwrap();
        assert!(import(
            root.path(),
            &store,
            &session,
            &receipt.id,
            &["a.txt".into()]
        )
        .unwrap_err()
        .to_string()
        .contains("drift"));
        assert_eq!(
            fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "before\n"
        );
        let mut other = session.clone();
        other.active_branch = "other".into();
        assert!(load(&store, &other, &receipt.id).is_err());
        store.finish_execution(&session.id, &owner, None).unwrap();
    }
    #[test]
    fn deletions_binary_and_mode_changes_are_visible_but_not_importable() {
        use std::os::unix::fs::PermissionsExt;
        let (root, copy, store, session, mut receipt) = fixture();
        fs::remove_file(copy.path().join("a.txt")).unwrap();
        fs::set_permissions(copy.path().join("b.txt"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(copy.path().join("binary"), [0xff, 0xfe]).unwrap();
        inspect(root.path(), copy.path(), &mut receipt);
        assert_eq!(receipt.modifications, "present");
        assert_eq!(receipt.edits.len(), 3);
        assert!(receipt.edits.iter().all(|e| e.after.is_none()));
        publish(&store, &session, &receipt);
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        assert!(import(
            root.path(),
            &store,
            &session,
            &receipt.id,
            &["a.txt".into()]
        )
        .is_err());
        store.finish_execution(&session.id, &owner, None).unwrap();
    }
    #[test]
    fn source_drift_and_unreadable_results_never_become_clean_verification() {
        use std::os::unix::fs::symlink;
        let (root, copy, _, _, mut receipt) = fixture();
        fs::write(root.path().join("a.txt"), "manual").unwrap();
        fs::write(copy.path().join("a.txt"), "formatter").unwrap();
        inspect(root.path(), copy.path(), &mut receipt);
        assert!(!receipt.live_inputs_match);
        assert!(receipt.edits.is_empty());
        assert_eq!(receipt.modifications, "unknown");
        symlink("/etc/passwd", copy.path().join("alias")).unwrap();
        inspect(root.path(), copy.path(), &mut receipt);
        assert_eq!(receipt.outcome, "unknown");
        assert_eq!(receipt.modifications, "unknown");
    }
    #[test]
    fn missing_runtime_hints_are_diagnostic_only_and_ignore_relative_path() {
        use std::os::unix::fs::PermissionsExt;
        let host = tempfile::tempdir().unwrap();
        let candidate = host.path().join("fake-runtime");
        std::fs::write(&candidate, "#!/bin/sh\nexit 99\n").unwrap();
        std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o755)).unwrap();
        let required = vec!["fake-runtime".into(), "missing-runtime".into()];
        let path = std::env::join_paths([Path::new("relative"), host.path()]).unwrap();
        let hints = runtime_hints(&required, "Required runtime unavailable: fake-runtime\nRequired runtime unavailable: missing-runtime\n", Some(&path));
        assert!(hints.contains("host candidate"));
        assert!(hints.contains(&format!("{candidate:?}")));
        assert!(hints.contains("no executable found"));
        assert!(runtime_hints(&required, "namespace unavailable", Some(&path)).is_empty());
        assert!(runtime_hints(
            &required,
            "Required runtime unavailable: fake-runtime",
            Some(std::ffi::OsStr::new("relative"))
        )
        .contains("no executable found"));
    }

    #[test]
    fn recipe_authority_is_fingerprinted_and_runtime_names_cannot_inject_shell() {
        let mut recipe = Recipe {
            command: "python3 -m unittest".into(),
            requires: vec!["python3".into()],
            git: false,
            preparation: None,
        };
        recipe.validate().unwrap();
        let original = recipe.fingerprint();
        recipe.git = true;
        assert_ne!(original, recipe.fingerprint());
        recipe.requires.push("python3; touch escape".into());
        assert!(recipe.validate().is_err());
        recipe.requires = vec!["--".into()];
        assert!(recipe.validate().is_err());
        assert!(crate::tools::parse_action("action: tool\ntool: check\ninput: tests").is_ok());
        assert!(crate::tools::parse_action("action: tool\ntool: check").is_err());
    }
    #[tokio::test]
    #[ignore = "requires Bubblewrap user namespaces; verifies setup/check failure timing boundaries"]
    async fn live_phase_timings_preserve_failed_setup_and_failed_check() {
        let (root, _, store, session, _) = fixture();
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        for setup_fails in [true, false] {
            let recipe = Recipe {
                command: "exit 7".into(),
                requires: vec![],
                git: false,
                preparation: Some(crate::check_dependencies::Preparation {
                    artifacts: vec![],
                    run: Some(if setup_fails { "exit 9" } else { "true" }.into()),
                }),
            };
            let receipt = run(
                Request {
                    root: root.path(),
                    store: &store,
                    session: &session,
                    step: "verify",
                    name: "timing",
                    recipe: &recipe,
                    stop: &stop,
                },
                || Ok(()),
            )
            .await
            .unwrap();
            assert!(receipt.phase_ms.contains_key("offline_setup"));
            assert!(receipt.phase_ms.contains_key("change_inspection"));
            assert_eq!(
                receipt.phase_ms.contains_key("check_execution"),
                !setup_fails
            );
            assert_eq!(
                receipt.launch,
                if setup_fails {
                    "not_started"
                } else {
                    "started"
                }
            );
            assert_eq!(
                receipt.outcome,
                if setup_fails { "unavailable" } else { "failed" }
            );
            assert!(receipt
                .phase_ms
                .values()
                .all(|ms| ms.is_finite() && *ms >= 0.0));
            store
                .event(
                    &session,
                    Some("verify"),
                    "verification.workspace",
                    &receipt.report(),
                )
                .unwrap();
            let recorded = load(&store, &session, &receipt.id).unwrap();
            assert_eq!(
                recorded.phase_ms.keys().collect::<Vec<_>>(),
                receipt.phase_ms.keys().collect::<Vec<_>>()
            );
        }
        store.finish_execution(&session.id, &owner, None).unwrap();
    }

    #[tokio::test]
    #[ignore = "requires Bubblewrap user namespaces; asserts actual missing-runtime diagnostics"]
    async fn live_preflight_reports_all_missing_runtimes_before_approval() {
        let (root, _, store, session, _) = fixture();
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        let recipe = Recipe {
            command: "touch should-not-run".into(),
            requires: vec![
                "greenlight-fixture-missing-one".into(),
                "greenlight-fixture-missing-two".into(),
            ],
            git: false,
            preparation: None,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let mut authorized = false;
        let receipt = run(
            Request {
                root: root.path(),
                store: &store,
                session: &session,
                step: "verify",
                name: "missing",
                recipe: &recipe,
                stop: &stop,
            },
            || {
                authorized = true;
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(!authorized);
        assert_eq!(receipt.outcome, "unavailable");
        assert_eq!(receipt.launch, "not_started");
        for name in &recipe.requires {
            assert!(
                receipt
                    .diagnostic
                    .contains(&format!("Required runtime unavailable: {name}")),
                "{}",
                receipt.diagnostic
            );
        }
        assert!(receipt.diagnostic.contains("no executable found"));
        assert!(!root.path().join("should-not-run").exists());
        store.finish_execution(&session.id, &owner, None).unwrap();
    }

    #[tokio::test]
    async fn real_profile_either_runs_in_copy_or_reports_exact_preflight_unavailability() {
        let (root, _, store, session, _) = fixture();
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        let recipe = Recipe {
            command: "printf 'formatted\\n' > a.txt".into(),
            requires: vec![],
            git: false,
            preparation: None,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let mut authorized = false;
        let receipt = run(
            Request {
                root: root.path(),
                store: &store,
                session: &session,
                step: "verify",
                name: "formatter",
                recipe: &recipe,
                stop: &stop,
            },
            || {
                authorized = true;
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "before\n"
        );
        assert!(receipt.phase_ms.contains_key("workspace_setup"));
        assert!(receipt.phase_ms.contains_key("preflight"));
        assert!(receipt
            .phase_ms
            .values()
            .all(|value| value.is_finite() && *value >= 0.0));
        match receipt.outcome.as_str() {
            "unavailable" => {
                assert!(!receipt.phase_ms.contains_key("check_execution"));
                assert_eq!(receipt.launch, "not_started");
                assert!(!authorized);
                assert!(receipt.diagnostic.contains("Preflight"));
            }
            "passed" => {
                assert_eq!(receipt.launch, "started");
                assert!(authorized);
                assert_eq!(receipt.modifications, "present");
                for phase in [
                    "approval_wait",
                    "dependency_staging",
                    "source_capture_and_recheck",
                    "check_execution",
                    "change_inspection",
                ] {
                    assert!(
                        receipt.phase_ms.contains_key(phase),
                        "missing phase {phase}"
                    );
                }
                assert!(!receipt.phase_ms.contains_key("offline_setup"));
            }
            other => panic!("unexpected outcome: {other}\n{}", receipt.diagnostic),
        }
        store.finish_execution(&session.id, &owner, None).unwrap();
    }
}
