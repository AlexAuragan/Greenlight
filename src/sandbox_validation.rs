//! Repeatable, provider-free validation of real sandbox paths. Synthetic
//! fixtures never become a host-execution fallback when namespaces are denied.
use crate::{
    store::{Session, Store},
    verification_workspace::{Receipt, Recipe, Request},
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const REVISION: &str = "sandbox-fixtures-v3";
const PROJECT_FILES: &str = include_str!("sandbox_validation_projects.json");
const PUBLIC_PINS: &str = include_str!("sandbox_validation_dependencies.json");
const PREFIX: &str = "greenlight-validation-";
const PINNED: &[u8] = b"trusted local dependency fixture\n";
#[derive(Clone, Copy)]
enum Scenario {
    Isolation,
    Pipeline,
    ShellBoundaries,
    NetworkIpc,
    Rust,
    Python,
    Node,
    Hook,
    PreparedNode,
    PreparedHook,
    NpmProject,
    PythonHookProject,
    Venv,
    Formatter,
    Drift,
    FailingCheck,
    OutputLimit,
    Stop,
}
const SCENARIOS: &[(Scenario, &str)] = &[
    (Scenario::Isolation, "isolation_startup"),
    (Scenario::Pipeline, "inspection_head_pipeline"),
    (Scenario::ShellBoundaries, "readonly_secret_path_boundaries"),
    (Scenario::NetworkIpc, "network_and_ipc_sentinels"),
    (Scenario::Rust, "rust_offline_tests"),
    (Scenario::Python, "python_stdlib_tests"),
    (Scenario::Node, "node_syntax_and_execution"),
    (Scenario::Hook, "local_precommit_system_hook"),
    (Scenario::Venv, "pinned_fixture_and_fresh_venv"),
    (Scenario::Formatter, "formatter_import_recheck"),
    (Scenario::Drift, "external_drift_import_refusal"),
    (Scenario::FailingCheck, "failed_check_is_not_pass"),
    (Scenario::OutputLimit, "output_limit_cleanup"),
    (Scenario::Stop, "setsid_stop_cleanup"),
];
#[derive(Serialize)]
pub struct Report {
    schema: &'static str,
    fixture_revision: &'static str,
    generated_at: String,
    package_version: &'static str,
    build_profile: &'static str,
    timing_scope: &'static str,
    binary_sha256: Option<String>,
    os: &'static str,
    arch: &'static str,
    kernel: Option<String>,
    bubblewrap_version: String,
    pub gate_status: String,
    public_dependencies: bool,
    dependency_projects: bool,
    public_pins: serde_json::Value,
    artifact_origin: &'static str,
    warm_samples: u8,
    cold_samples: u8,
    requested_counts_meet_recommendation: bool,
    recommended_sample_count_met: bool,
    fixture_sha256: String,
    fixture_public_files: usize,
    fixture_public_bytes: usize,
    cache_definitions: BTreeMap<&'static str, &'static str>,
    unmeasured: Vec<&'static str>,
    remaining_validation: Vec<&'static str>,
    retained_paths: Vec<String>,
    cases: Vec<Case>,
}
#[derive(Serialize)]
struct Case {
    name: String,
    expectation: &'static str,
    status: String,
    reason: Option<String>,
    samples: Vec<Sample>,
    summaries: BTreeMap<String, Summary>,
}
#[derive(Serialize)]
struct Sample {
    cache: String,
    index: u8,
    elapsed_ms: f64,
    gate: String,
    observed: String,
    diagnostic_excerpt: String,
    diagnostic_sha256: String,
    cleanup_confirmed: bool,
    source_files: Option<usize>,
    source_bytes: Option<usize>,
    stop_to_cleanup_ms: Option<f64>,
    phase_ms: BTreeMap<String, f64>,
    executions: BTreeMap<String, crate::sandbox::pipeline::Measurements>,
    cache_storage: Option<crate::verification_storage::Observation>,
    retained_paths: Vec<String>,
}
#[derive(Serialize)]
struct Summary {
    samples: usize,
    met_expectations: usize,
    blocked: usize,
    failed: usize,
    all_p50_ms: Option<f64>,
    all_p95_ms: Option<f64>,
    met_expectations_p50_ms: Option<f64>,
    met_expectations_p95_ms: Option<f64>,
    phase_ms: BTreeMap<String, PhaseSummary>,
    execution_statistics: BTreeMap<String, MeasurementSummary>,
}
#[derive(Serialize)]
struct PhaseSummary {
    measured_samples: usize,
    p50_ms: f64,
    p95_ms: f64,
}
#[derive(Serialize)]
struct MeasurementSummary {
    measured_samples: usize,
    unit: &'static str,
    p50: f64,
    p95: f64,
}
struct Observation {
    gate: &'static str,
    observed: String,
    diagnostic: String,
    files: Option<usize>,
    bytes: Option<usize>,
    stop_ms: Option<f64>,
    phase_ms: BTreeMap<String, f64>,
    executions: BTreeMap<String, crate::sandbox::pipeline::Measurements>,
    cache_storage: Option<crate::verification_storage::Observation>,
}
impl Observation {
    fn new(gate: &'static str, observed: impl Into<String>, diagnostic: impl Into<String>) -> Self {
        Self {
            gate,
            observed: observed.into(),
            diagnostic: diagnostic.into(),
            files: None,
            bytes: None,
            stop_ms: None,
            phase_ms: BTreeMap::new(),
            executions: BTreeMap::new(),
            cache_storage: None,
        }
    }
}
fn expectation(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::Isolation => {
            "Namespace startup succeeds; source is read-only, tmp/cache writable"
        }
        Scenario::Pipeline => {
            "Exact first line, all stages exit zero, expected head short-read drain"
        }
        Scenario::ShellBoundaries => {
            "Private project paths, original Git and outside symlink target stay inaccessible"
        }
        Scenario::NetworkIpc => {
            "Host TCP/Unix positive controls work; sandbox cannot reach either sentinel"
        }
        Scenario::PreparedNode | Scenario::PreparedHook | Scenario::NpmProject | Scenario::PythonHookProject => "Pinned artifact preparation verifies; fresh offline provisioned runtime check passes without live edits (artifact_origin distinguishes download from external supply)",
        Scenario::Rust | Scenario::Python | Scenario::Node | Scenario::Hook => {
            "Fixed offline check passes without changing live source"
        }
        Scenario::Venv => {
            "Synthetic pinned bytes are readonly; each check starts with a fresh private venv"
        }
        Scenario::Formatter => {
            "Copy edits only; explicit native import; fresh unchanged-input recheck passes"
        }
        Scenario::Drift => "Manual input drift refuses import and preserves the manual edit",
        Scenario::FailingCheck => "Intentional exit 7 is a failed check, never a pass",
        Scenario::OutputLimit => "Capture overflow is incomplete/unknown, with confirmed cleanup",
        Scenario::Stop => "Confirmed session-escaped child is stopped; owned cleanup completes",
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
fn files() -> BTreeMap<String, String> {
    let mut files = BTreeMap::from([
        ("data.txt".into(),"original\n".into()), ("other.txt".into(),"unchanged\n".into()),
        ("values.txt".into(),"fixture_value\n".repeat(10_000)),
        ("Cargo.toml".into(),"[package]\nname = \"greenlight-validation-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n".into()),
        ("Cargo.lock".into(),"version = 3\n\n[[package]]\nname = \"greenlight-validation-fixture\"\nversion = \"0.0.0\"\n".into()),
        ("src/lib.rs".into(),"pub fn add(a: u32,b: u32)->u32 {a+b}\n#[test] fn sum(){assert_eq!(add(2,2),4);}\n".into()),
        ("test_fixture.py".into(),"import unittest\nclass Fixture(unittest.TestCase):\n def test_sum(self): self.assertEqual(2+2,4)\n".into()),
        ("app.js".into(),"if (2+2 !== 4) throw Error('fixture'); console.log('fixture ok');\n".into()),
        ("scripts/check.py".into(),"from pathlib import Path\nassert Path('data.txt').read_text() == 'original\\n'\n".into()),
        (".pre-commit-config.yaml".into(),"repos:\n  - repo: local\n    hooks:\n      - id: fixture\n        name: local system fixture\n        language: system\n        entry: python3 scripts/check.py\n        pass_filenames: false\n        always_run: true\n".into()),
    ]);
    files.extend(
        serde_json::from_str::<BTreeMap<String, String>>(PROJECT_FILES)
            .expect("compiled project fixture files"),
    );
    files
}
struct Fixture {
    store: Store,
    session: Session,
    root: Option<tempfile::TempDir>,
    outside: Option<tempfile::TempDir>,
    keep: bool,
    artifact_directory: Option<crate::anchored_fs::Dir>,
}
impl Fixture {
    fn new(artifact_directory: Option<&Path>) -> Result<Self> {
        let root = tempfile::Builder::new().prefix(PREFIX).tempdir_in("/tmp")?;
        let outside = tempfile::Builder::new()
            .prefix("greenlight-sentinel-")
            .tempdir_in("/tmp")?;
        fs::write(outside.path().join("sentinel"), b"BENIGN_OUTSIDE_SENTINEL")?;
        let store = Store::open(root.path())?;
        let session = store.create(
            "validation-fixture",
            "Synthetic sandbox validation",
            "verify",
        )?;
        let fixture = Self {
            store,
            session,
            root: Some(root),
            outside: Some(outside),
            keep: false,
            artifact_directory: artifact_directory
                .map(crate::anchored_fs::Dir::open)
                .transpose()?,
        };
        fixture.reset()?;
        Ok(fixture)
    }
    fn path(&self) -> &Path {
        self.root.as_ref().unwrap().path()
    }
    fn outside(&self) -> &Path {
        self.outside.as_ref().unwrap().path()
    }
    fn owned_paths(&self) -> Vec<String> {
        self.root
            .as_ref()
            .into_iter()
            .chain(self.outside.as_ref())
            .map(|dir| dir.path().display().to_string())
            .collect()
    }
    fn reset(&self) -> Result<()> {
        for (name, content) in files() {
            let path = self.path().join(name);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, content)?;
        }
        fs::write(
            self.path().join(".env"),
            b"BENIGN_SECRET_SENTINEL=not-a-real-secret\n",
        )?;
        fs::create_dir_all(self.path().join(".git"))?;
        fs::write(
            self.path().join(".git/original"),
            b"BENIGN_ORIGINAL_GIT_SENTINEL",
        )?;
        fs::write(
            self.path().join(".greenlight/config.yaml"),
            b"provider:\n  model: unused-validation-fixture\n",
        )?;
        Ok(())
    }
    fn source_matches(&self, overrides: &[(&str, &str)]) -> Result<bool> {
        for (path, expected) in files() {
            let expected = overrides
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, value)| *value)
                .unwrap_or(&expected);
            if fs::read_to_string(self.path().join(path))? != expected {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn request<'a>(&'a self, recipe: &'a Recipe, stop: &'a Arc<AtomicBool>) -> Request<'a> {
        Request {
            root: self.path(),
            store: &self.store,
            session: &self.session,
            step: "verify",
            name: "validation",
            recipe,
            stop,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.keep || crate::sandbox_job::ensure_clean(&self.store.directory_fd).is_err() {
            for directory in [&mut self.root, &mut self.outside] {
                if let Some(directory) = directory.take() {
                    eprintln!(
                        "Retained validation fixture: {}",
                        directory.keep().display()
                    );
                }
            }
        }
    }
}
fn recipe(command: impl Into<String>, requires: &[&str], git: bool) -> Recipe {
    Recipe {
        command: command.into(),
        requires: requires.iter().map(|s| s.to_string()).collect(),
        git,
        preparation: None,
    }
}
fn public_recipe(scenario: Scenario) -> Result<Recipe> {
    let pins: BTreeMap<String, Vec<crate::check_dependencies::Artifact>> =
        serde_json::from_str(PUBLIC_PINS)?;
    let (mut recipe, mut key, mut setup) = if matches!(
        scenario,
        Scenario::PreparedNode | Scenario::NpmProject
    ) {
        (recipe("/cache/node/bin/node --version && /cache/node/bin/node --check app.js && /cache/node/bin/node app.js", &["tar", "xz"], false), "node", "mkdir /cache/node && tar -xJf /deps/node-v22.15.0-linux-x64.tar.xz --strip-components=1 -C /cache/node".to_string())
    } else {
        let artifacts = &pins["precommit"];
        let lock = artifacts
            .iter()
            .map(|a| {
                let mut parts = a.name.split('-');
                format!(
                    "{}=={} --hash=sha256:{}",
                    parts.next().unwrap(),
                    parts.next().unwrap(),
                    a.sha256
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let setup = format!("python3 -m venv --without-pip /cache/venv && printf '%s\\n' {} > /cache/requirements.lock && PYTHONPATH=/deps/pip-25.1.1-py3-none-any.whl /cache/venv/bin/python -m pip --isolated install --no-index --no-deps --require-hashes --find-links=/deps -r /cache/requirements.lock", quote(&lock));
        (recipe("/cache/venv/bin/pre-commit --version && test ! -e .git/original && test -z \"$(git remote)\" && /cache/venv/bin/pre-commit run --all-files", &["python3", "git"], true), "precommit", setup)
    };
    if matches!(scenario, Scenario::NpmProject) {
        key = "npm_project";
        setup.push_str(r#" && export PATH=/cache/node/bin:$PATH && npm --version && npm cache add /deps/is-odd-3.0.1.tgz --offline --ignore-scripts --cache=/cache/npm && npm cache add /deps/is-number-6.0.0.tgz --offline --ignore-scripts --cache=/cache/npm && npm ci --offline --ignore-scripts --no-audit --no-fund --cache=/cache/npm"#);
        recipe.command = r#"export PATH=/cache/node/bin:$PATH && test ! -e /cache/unexpected-lifecycle && npm test --offline --ignore-scripts && test -s /cache/npm-test-ran && mkdir /cache/missing-project && cp package.json package-lock.json /cache/missing-project/ && npm cache add /deps/is-odd-3.0.1.tgz --offline --ignore-scripts --cache=/cache/empty-npm && if npm ci --prefix /cache/missing-project --offline --ignore-scripts --no-audit --no-fund --cache=/cache/empty-npm > /cache/missing-npm.log 2>&1; then echo 'Unexpected success without dependencies'; exit 92; else grep ENOTCACHED /cache/missing-npm.log && grep is-number /cache/missing-npm.log; fi && test ! -e /cache/unexpected-lifecycle && printf 'Offline missing-dependency failure confirmed\n'"#.into();
        recipe.requires.push("grep".into());
    }
    if matches!(scenario, Scenario::PythonHookProject) {
        key = "python_hook";
        recipe
            .requires
            .extend(["tar".into(), "cmp".into(), "grep".into()]);
        setup.push_str(r#" && mkdir /cache/hook-repo && tar -xzf /deps/pyupgrade-ce40a160603ab0e7d9c627ae33d7ef3906e2d2b2.tar.gz --strip-components=1 -C /cache/hook-repo && test ! -e /cache/hook-repo/.git && git -C /cache/hook-repo -c init.templateDir= -c core.hooksPath=/dev/null init -q && git -C /cache/hook-repo -c core.hooksPath=/dev/null add -f -- . && GIT_AUTHOR_DATE=2025-03-01T00:00:00Z GIT_COMMITTER_DATE=2025-03-01T00:00:00Z git -C /cache/hook-repo -c user.name=Fixture -c user.email=fixture@example.invalid -c core.hooksPath=/dev/null commit -qm 'Pinned pyupgrade archive fixture' && git -C /cache/hook-repo tag fixture-source-v3.19.1"#);
        recipe.command = r#"export PATH=/cache/venv/bin:$PATH PIP_FIND_LINKS=/deps PIP_NO_INDEX=1 VIRTUALENV_NO_PERIODIC_UPDATE=1 VIRTUALENV_DOWNLOAD=0 GIT_ALLOW_PROTOCOL=file && test ! -e .git/original && pre-commit run --config project-hooks.yaml --all-files && /cache/venv/bin/python verify-hook-environment.py && cp test_fixture.py /cache/original-python && printf "name = 'world'\nmessage = '{}'.format(name)\n" > test_fixture.py && cp test_fixture.py /cache/bad-python && if pre-commit run --config project-hooks.yaml --files test_fixture.py > /cache/negative-hook.log 2>&1; then echo 'Unexpected pass for outdated Python syntax'; exit 93; else grep 'files were modified' /cache/negative-hook.log; fi && ! cmp -s /cache/bad-python test_fixture.py && cp /cache/original-python test_fixture.py && printf 'Python hook rewrite failure confirmed and fixture restored\n'"#.into();
    }
    recipe.preparation = Some(crate::check_dependencies::Preparation {
        artifacts: pins[key].clone(),
        run: Some(setup),
    });
    recipe.validate()?;
    Ok(recipe)
}

async fn check(fixture: &Fixture, recipe: &Recipe, stop: &Arc<AtomicBool>) -> Result<Receipt> {
    let receipt =
        crate::verification_workspace::run(fixture.request(recipe, stop), || Ok(())).await?;
    fixture.store.event(
        &fixture.session,
        Some("verify"),
        "verification.workspace",
        &receipt.report(),
    )?;
    Ok(receipt)
}
fn from_receipt(receipt: &Receipt, met: bool) -> Observation {
    let mut observation = Observation::new(
        if receipt.outcome == "unavailable" {
            "blocked"
        } else if met {
            "passed"
        } else {
            "failed"
        },
        format!(
            "check={} modifications={} launch={}",
            receipt.outcome, receipt.modifications, receipt.launch
        ),
        format!(
            "{}\nOffline setup: {:?}",
            receipt.diagnostic, receipt.preparation_diagnostic
        ),
    );
    observation.phase_ms = receipt.phase_ms.clone();
    observation.executions = receipt.executions.clone();
    observation.cache_storage = receipt.cache_storage.clone();
    observation.files = Some(receipt.inputs.len());
    let known = files();
    observation.bytes = receipt
        .inputs
        .keys()
        .map(|path| known.get(path).map(String::len))
        .sum();
    observation
}
fn clean_pass(receipt: &Receipt) -> bool {
    receipt.outcome == "passed" && receipt.modifications == "none" && receipt.live_inputs_match
}

async fn perform(fixture: &Fixture, scenario: Scenario) -> Result<Observation> {
    let stop = Arc::new(AtomicBool::new(false));
    match scenario {
        Scenario::Isolation => {
            match crate::sandbox::Sandbox::open(fixture.path())?.run("test ! -w /workspace && test -w /cache && test -w /tmp && printf 'isolation ready\\n'").await {
                Ok(output) => Ok(Observation::new(if output.status.success(){"passed"}else{"failed"},format!("{}",output.status),format!("{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr)))),
                Err(error) if error.is::<crate::sandbox_job::CleanupIncomplete>() => Err(error),
                Err(error) => Ok(Observation::new("blocked","sandbox setup unavailable",format!("{error:#}"))),
            }
        }
        Scenario::Pipeline => {
            let plan = crate::project_inspection::parse(fixture.path(),"cat values.txt | head -n 1").context("Invalid fixed validation pipeline")?;
            let view = plan.snapshot(fixture.path())?;
            let output = crate::sandbox::Sandbox::project_inspection(&view,fixture.path())?.inspect_pipeline(&plan,&stop).await?;
            let met = output.output.status.success() && output.output.stdout == b"fixture_value\n" && output.summary.contains("expected short read");
            let mut observation = Observation::new(if met {"passed"}else{"failed"},format!("{}",output.output.status),output.summary);
            observation.executions.insert("inspection_pipeline".into(), output.measurements);
            Ok(observation)
        }
        Scenario::ShellBoundaries => {
            let link = fixture.path().join("outside-link");
            std::os::unix::fs::symlink(fixture.outside().join("sentinel"),&link)?;
            let command = format!("test ! -w /workspace && test ! -s /workspace/.env && test ! -s /workspace/.greenlight/config.yaml && test ! -e /workspace/.git/original && test ! -e /workspace/outside-link && test ! -e {} && test ! -e /home/agent/.ssh && test -z \"${{DEEPINFRA_API_KEY:-}}\"",quote(fixture.outside().join("sentinel").to_str().context("Non-UTF8 fixture path")?));
            let result = crate::sandbox::Sandbox::open(fixture.path())?.run(&command).await;
            fs::remove_file(link)?;
            let output = result?;
            Ok(Observation::new(if output.status.success(){"passed"}else{"failed"},format!("{}",output.status),String::from_utf8_lossy(&output.stderr)))
        }
        Scenario::NetworkIpc => {
            use std::net::{TcpListener,TcpStream};
            use std::os::unix::net::{UnixListener,UnixStream};
            let mut selected = None;
            for _ in 0..16 {
                let listener = TcpListener::bind("127.0.0.1:0")?;
                if ![22,8000,8001,8080,8081,8082,8083,8085,9222,31460,50002].contains(&listener.local_addr()?.port()) {
                    selected = Some(listener); break;
                }
            }
            let tcp = selected.context("No free non-reserved loopback fixture port")?;
            let address = tcp.local_addr()?;
            let _positive = TcpStream::connect_timeout(&address,Duration::from_secs(1))?; drop(tcp.accept()?); drop(_positive); tcp.set_nonblocking(true)?;
            let socket = fixture.outside().join("service.sock");
            let unix = UnixListener::bind(&socket)?;
            let _positive = UnixStream::connect(&socket)?; drop(unix.accept()?); drop(_positive); unix.set_nonblocking(true)?;
            let python = format!("import socket\nfor family,target in [(socket.AF_INET,('127.0.0.1',{})),(socket.AF_UNIX,{})]:\n s=socket.socket(family); s.settimeout(.5)\n try:\n  s.connect(target)\n except OSError:\n  pass\n else:\n  raise AssertionError('host service reachable')\n finally:\n  s.close()\n",address.port(),serde_json::to_string(socket.to_str().unwrap())?);
            let receipt = check(fixture,&recipe(format!("python3 --version && python3 -c {}",quote(&python)),&["python3"],false),&stop).await?;
            let accepted = matches!(tcp.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock) && matches!(unix.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock);
            fs::remove_file(socket)?;
            Ok(from_receipt(&receipt,clean_pass(&receipt) && accepted))
        }
        Scenario::Rust | Scenario::Python | Scenario::Node | Scenario::Hook => {
            let recipe = match scenario {
                Scenario::Rust => recipe("cargo --version && rustc --version && cargo test --locked --offline",&["cargo","rustc","cc"],false),
                Scenario::Python => recipe("python3 --version && python3 -m unittest test_fixture.py",&["python3"],false),
                Scenario::Node => recipe("node --version && node --check app.js && node app.js",&["node"],false),
                _ => recipe("pre-commit --version && test ! -e .git/original && test -z \"$(git remote)\" && git ls-files --error-unmatch data.txt && pre-commit run --all-files",&["pre-commit","python3","git"],true),
            };
            let receipt = check(fixture,&recipe,&stop).await?;
            Ok(from_receipt(&receipt,clean_pass(&receipt)))
        }
        Scenario::PreparedNode | Scenario::PreparedHook | Scenario::NpmProject | Scenario::PythonHookProject => {
            let recipe = public_recipe(scenario)?;
            let supplied_started = Instant::now();
            if let Some(directory) = &fixture.artifact_directory {
                let objects = fixture.store.directory_fd.create_dirs(Path::new("check-dependencies/objects"))?;
                for artifact in &recipe.preparation.as_ref().unwrap().artifacts {
                    let mut bytes = Vec::new();
                    directory.open_file(Path::new(&artifact.name))?.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                    if bytes.len() > 32 * 1024 * 1024 || digest(&bytes) != artifact.sha256 {
                        bail!("Externally supplied artifact {} failed size/SHA-256 validation; no broker fallback", artifact.name);
                    }
                    objects.target(Path::new(&format!("{}.bin",artifact.sha256)))?.atomic_write(&bytes)?;
                }
            }
            let supplied_ms = supplied_started.elapsed().as_secs_f64() * 1000.0;
            let preparation_started = Instant::now();
            let prepared = crate::check_dependencies::prepare(fixture.request(&recipe, &stop), || Ok(())).await;
            let preparation_ms = preparation_started.elapsed().as_secs_f64() * 1000.0;
            let preparation = match prepared {
                Ok(text) => text,
                Err(error) if error.is::<crate::sandbox_job::CleanupIncomplete>() => return Err(error),
                Err(error) => {
                    let mut observation = Observation::new("blocked", "Public artifact preparation unavailable; offline check not started", format!("{error:#}"));
                    observation.phase_ms.insert("artifact_preparation".into(), preparation_ms);
                    if fixture.artifact_directory.is_some() {
                        observation.phase_ms.insert("external_artifact_verification_and_copy".into(), supplied_ms);
                    }
                    return Ok(observation);
                }
            };
            let receipt = check(fixture, &recipe, &stop).await?;
            let mut observation = from_receipt(&receipt, clean_pass(&receipt));
            observation.phase_ms.insert("artifact_preparation".into(), preparation_ms);
            if fixture.artifact_directory.is_some() {
                observation.phase_ms.insert("external_artifact_verification_and_copy".into(), supplied_ms);
            }
            observation.diagnostic = format!("{preparation}\n{}", observation.diagnostic);
            Ok(observation)
        }
        Scenario::Venv => {
            // White-box fixture seeding, NOT a network fetch or a claim about
            // production TLS/DNS provenance. No downloader is called here.
            let artifact = crate::check_dependencies::Artifact { name:"pinned.bin".into(),url:"https://fixture.invalid/pinned.bin".into(),sha256:digest(PINNED) };
            let mut recipe = recipe("/cache/venv/bin/python -c 'from pathlib import Path; assert Path(\"/cache/copied\").read_bytes() == b\"trusted local dependency fixture\\n\"'",&["python3"],false);
            recipe.preparation = Some(crate::check_dependencies::Preparation { artifacts:vec![artifact.clone()],run:Some("test ! -e /cache/venv && python3 -m venv --without-pip /cache/venv && cp /deps/pinned.bin /cache/copied && if printf poison > /deps/pinned.bin; then exit 91; fi".into()) });
            let objects = fixture.store.directory_fd.create_dirs(Path::new("check-dependencies/objects"))?;
            let object_path = format!("{}.bin",artifact.sha256);
            if objects.open_file(Path::new(&object_path)).is_err() { objects.target(Path::new(&object_path))?.atomic_write(PINNED)?; }
            let manifest = serde_json::json!({"fingerprint":recipe.fingerprint(),"artifacts":[artifact],"bytes":PINNED.len(),"prepared_at":"synthetic local fixture, not a download"});
            let manifests = fixture.store.directory_fd.create_dirs(Path::new("check-dependencies/manifests"))?;
            let manifest_path = format!("{}.json",recipe.fingerprint());
            if manifests.open_file(Path::new(&manifest_path)).is_err() { manifests.target(Path::new(&manifest_path))?.atomic_write(&serde_json::to_vec(&manifest)?)?; }
            let receipt = check(fixture,&recipe,&stop).await?;
            Ok(from_receipt(&receipt,clean_pass(&receipt) && receipt.dependencies.len()==1 && receipt.preparation_diagnostic.is_some()))
        }
        Scenario::Formatter => {
            let receipt = check(fixture,&recipe("printf 'formatted\\n' > data.txt",&[],false),&stop).await?;
            let mut met = receipt.outcome=="passed" && receipt.modifications=="present" && receipt.edits.len()==1 && fixture.source_matches(&[])?;
            if met {
                crate::verification_workspace::import(fixture.path(),&fixture.store,&fixture.session,&receipt.id,&["data.txt".into()])?;
                met &= fixture.source_matches(&[("data.txt","formatted\n")])?;
                let recheck = check(fixture,&recipe("test \"$(cat data.txt)\" = formatted",&["cat"],false),&stop).await?;
                met &= clean_pass(&recheck);
            }
            Ok(from_receipt(&receipt,met))
        }
        Scenario::Drift => {
            let marker = uuid::Uuid::new_v4().to_string();
            let recipe = recipe(format!("printf {marker} > /cache/ready; while test ! -e /cache/continue; do sleep .01; done; printf 'formatted\\n' > data.txt"), &["sleep"], false);
            let done = std::cell::Cell::new(false);
            let execute = async { let result = check(fixture, &recipe, &stop).await; done.set(true); result };
            let edit = async {
                let result = async {
                    let runs = fixture.store.directory_fd.create_dirs(Path::new("verification-runs"))?;
                    let deadline = Instant::now() + Duration::from_secs(5);
                    loop {
                        for entry in runs.entries()? {
                            let entry = entry?;
                            if !entry.directory { continue; }
                            let Ok(cache) = runs.open_dir(Path::new(&entry.name)).and_then(|dir|dir.open_dir(Path::new("cache"))) else { continue; };
                            if cache.read_string(Path::new("ready"),128).is_ok_and(|s|s==marker) {
                                fs::write(fixture.path().join("other.txt"),"external change\n")?;
                                cache.target(Path::new("continue"))?.atomic_write(b"continue")?;
                                return Ok::<_,anyhow::Error>(true);
                            }
                        }
                        if done.get() || Instant::now() >= deadline { return Ok(false); }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }.await;
                if !matches!(result, Ok(true)) { stop.store(true,Ordering::SeqCst); }
                result
            };
            let (receipt, edited) = tokio::join!(execute, edit);
            let receipt = receipt?; let edited = edited?;
            let refused = crate::verification_workspace::import(fixture.path(), &fixture.store, &fixture.session, &receipt.id, &["data.txt".into()]).is_err();
            Ok(from_receipt(&receipt, edited && !receipt.live_inputs_match && refused && fixture.source_matches(&[("other.txt","external change\n")])?))
        }
        Scenario::FailingCheck | Scenario::OutputLimit => {
            let recipe = if matches!(scenario,Scenario::FailingCheck) { recipe("printf 'intentional fixture failure\\n' >&2; exit 7",&[],false) } else {recipe("head -c 1048576 /dev/zero",&["head"],false)};
            let receipt = check(fixture,&recipe,&stop).await?;
            let expected = if matches!(scenario,Scenario::FailingCheck) { receipt.outcome=="failed" && receipt.diagnostic.contains("exit status: 7") } else {receipt.outcome=="unknown" && receipt.diagnostic.contains("output limit")};
            Ok(from_receipt(&receipt,expected && fixture.source_matches(&[])?))
        }
        Scenario::Stop => {
            let marker = uuid::Uuid::new_v4().to_string();
            let command = format!("command -v setsid >/dev/null || exit 127; setsid /bin/sh -c 'trap \"\" TERM; printf {marker} > /cache/ready; sleep 30' & wait");
            let cache = fixture.store.directory_fd.create_dirs(Path::new("sandbox-cache"))?;
            let done = std::cell::Cell::new(false);
            let signal = async {
                let deadline = Instant::now()+Duration::from_secs(5);
                loop {
                    if cache.read_string(Path::new("ready"),128).is_ok_and(|s|s==marker) { break; }
                    if done.get() { return None; }
                    if Instant::now()>=deadline { stop.store(true,Ordering::SeqCst); return None; }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                let requested = Instant::now(); stop.store(true,Ordering::SeqCst); Some(requested)
            };
            let sandbox = crate::sandbox::Sandbox::open(fixture.path())?;
            let execute = async { let result = sandbox.run_stoppable(&command,&stop).await; done.set(true); result };
            let (result,requested) = tokio::join!(execute,signal);
            let mut observation = match result {
                Err(error) if error.is::<crate::sandbox_job::CleanupIncomplete>() => return Err(error),
                Err(error) => Observation::new(if requested.is_some() && format!("{error:#}").contains("stopped by user"){"passed"}else{"failed"},"user stop",format!("{error:#}")),
                Ok(output) => Observation::new(if output.status.code()==Some(127){"blocked"}else{"failed"},format!("{}",output.status),"Stop fixture did not reach expected owned-child cancellation"),
            };
            observation.stop_ms = requested.map(|start|start.elapsed().as_secs_f64()*1000.0);
            Ok(observation)
        }
    }
}

async fn sample(
    fixture: &mut Fixture,
    scenario: Scenario,
    mode: &str,
    index: u8,
    keep_failures: bool,
) -> Result<Sample> {
    fixture.reset()?;
    let owner = fixture
        .store
        .claim_tool_execution(&fixture.session.id, &fixture.session.active_branch)?;
    let start = Instant::now();
    let result = perform(fixture, scenario).await;
    let cleanup =
        fixture
            .store
            .finish_execution(&fixture.session.id, &owner, result.as_ref().err());
    drop(owner);
    let cleanup_confirmed =
        cleanup.is_ok() && crate::sandbox_job::ensure_clean(&fixture.store.directory_fd).is_ok();
    let mut observation = match result {
        Ok(o) => o,
        Err(e) => Observation::new("failed", "execution error", format!("{e:#}")),
    };
    if let Err(error) = cleanup {
        observation.gate = "failed";
        observation
            .diagnostic
            .push_str(&format!("\nOwner cleanup: {error:#}"));
    }
    if !cleanup_confirmed {
        observation.gate = "failed";
    }
    fixture.keep |= !cleanup_confirmed || keep_failures && observation.gate != "passed";
    let diagnostic_sha256 = digest(observation.diagnostic.as_bytes());
    // Preview only: full check receipts can be retained with --keep-failures.
    let diagnostic_excerpt = observation
        .diagnostic
        .chars()
        .take(4096)
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                format!("\\u{{{:04x}}}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect::<String>();
    Ok(Sample {
        cache: mode.into(),
        index,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        gate: observation.gate.into(),
        observed: observation.observed,
        diagnostic_excerpt,
        diagnostic_sha256,
        cleanup_confirmed,
        source_files: observation.files,
        source_bytes: observation.bytes,
        stop_to_cleanup_ms: observation.stop_ms,
        phase_ms: observation.phase_ms,
        executions: observation.executions,
        cache_storage: observation.cache_storage,
        retained_paths: if fixture.keep {
            fixture.owned_paths()
        } else {
            Vec::new()
        },
    })
}
fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    Some(
        values[((values.len() as f64 * p).ceil() as usize)
            .saturating_sub(1)
            .min(values.len() - 1)],
    )
}
fn summary(samples: &[Sample], mode: &str) -> Summary {
    let samples = samples
        .iter()
        .filter(|s| s.cache == mode)
        .collect::<Vec<_>>();
    let all = samples.iter().map(|s| s.elapsed_ms).collect::<Vec<_>>();
    let passed = samples
        .iter()
        .filter(|s| s.gate == "passed")
        .map(|s| s.elapsed_ms)
        .collect::<Vec<_>>();
    let mut phase_values: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for sample in &samples {
        for (phase, elapsed) in &sample.phase_ms {
            phase_values
                .entry(phase.clone())
                .or_default()
                .push(*elapsed);
        }
    }
    let mut measurement_values: BTreeMap<String, (&'static str, Vec<f64>)> = BTreeMap::new();
    let mut record = |key: String, unit: &'static str, value: f64| {
        measurement_values
            .entry(key)
            .or_insert_with(|| (unit, Vec::new()))
            .1
            .push(value);
    };
    for sample in &samples {
        if let Some(footprint) = sample
            .cache_storage
            .as_ref()
            .and_then(|observation| observation.footprint.as_ref())
        {
            for (key, unit, value) in [
                (
                    "unique_regular_files",
                    "files",
                    footprint.unique_regular_files as u64,
                ),
                (
                    "logical_regular_file_bytes",
                    "bytes",
                    footprint.logical_regular_file_bytes,
                ),
                (
                    "allocated_regular_file_bytes",
                    "bytes",
                    footprint.allocated_regular_file_bytes,
                ),
            ] {
                record(format!("cache.{key}"), unit, value as f64);
            }
        }
        for (phase, execution) in &sample.executions {
            for (key, value) in [
                ("first_stdout", execution.first_stdout_ms),
                ("first_stderr", execution.first_stderr_ms),
                ("elapsed", Some(execution.elapsed_ms)),
            ] {
                if let Some(value) = value {
                    record(format!("{phase}.{key}"), "ms", value);
                }
            }
            for (index, usage) in execution.stage_resources.iter().enumerate() {
                let Some(usage) = usage else {
                    continue;
                };
                for (key, unit, value) in [
                    ("user_cpu", "us", usage.user_cpu_us),
                    ("system_cpu", "us", usage.system_cpu_us),
                    (
                        "largest_child_maxrss",
                        "KiB",
                        usage.largest_child_maxrss_kib,
                    ),
                    (
                        "input_block_operations",
                        "operations",
                        usage.input_block_operations,
                    ),
                    (
                        "output_block_operations",
                        "operations",
                        usage.output_block_operations,
                    ),
                ] {
                    record(format!("{phase}.stage_{index}.{key}"), unit, value as f64);
                }
            }
        }
    }
    Summary {
        samples: samples.len(),
        met_expectations: passed.len(),
        blocked: samples.iter().filter(|s| s.gate == "blocked").count(),
        failed: samples.iter().filter(|s| s.gate == "failed").count(),
        all_p50_ms: percentile(&all, 0.5),
        all_p95_ms: percentile(&all, 0.95),
        met_expectations_p50_ms: percentile(&passed, 0.5),
        met_expectations_p95_ms: percentile(&passed, 0.95),
        execution_statistics: measurement_values
            .into_iter()
            .map(|(key, (unit, values))| {
                (
                    key,
                    MeasurementSummary {
                        measured_samples: values.len(),
                        unit,
                        p50: percentile(&values, 0.5).unwrap(),
                        p95: percentile(&values, 0.95).unwrap(),
                    },
                )
            })
            .collect(),
        phase_ms: phase_values
            .into_iter()
            .map(|(phase, values)| {
                (
                    phase,
                    PhaseSummary {
                        measured_samples: values.len(),
                        p50_ms: percentile(&values, 0.5).unwrap(),
                        p95_ms: percentile(&values, 0.95).unwrap(),
                    },
                )
            })
            .collect(),
    }
}
pub async fn run(
    warm: u8,
    cold: u8,
    keep_failures: bool,
    public_dependencies: bool,
    dependency_projects: bool,
    artifact_directory: Option<&Path>,
) -> Result<Report> {
    if !(1..=100).contains(&warm) || !(1..=20).contains(&cold) {
        bail!("Warm samples must be 1..100 and cold samples 1..20");
    }
    if public_dependencies && std::env::consts::ARCH != "x86_64" {
        bail!("Public runtime fixtures are pinned for Linux x86_64; no alternative artifact is selected automatically");
    }
    if dependency_projects && !public_dependencies {
        bail!("Dependency projects require explicit public-dependencies mode");
    }
    if artifact_directory.is_some() && !public_dependencies {
        bail!("External artifact directories require explicit public-dependencies mode");
    }
    let scenarios: Vec<_> = if dependency_projects {
        vec![
            (Scenario::Isolation, "isolation_startup"),
            (Scenario::NpmProject, "npm_dependency_project"),
            (Scenario::PythonHookProject, "python_hook_project"),
        ]
    } else {
        SCENARIOS
            .iter()
            .map(|&(scenario, name)| match (public_dependencies, scenario) {
                (true, Scenario::Node) => (Scenario::PreparedNode, "public_node_offline"),
                (true, Scenario::Hook) => (Scenario::PreparedHook, "public_precommit_offline"),
                _ => (scenario, name),
            })
            .collect()
    };
    let public = files();
    let mut report = Report {
        schema: "greenlight.sandbox-validation.v1",
        fixture_revision: REVISION,
        generated_at: chrono::Utc::now().to_rfc3339(),
        package_version: env!("CARGO_PKG_VERSION"),
        build_profile: if cfg!(debug_assertions) { "debug" } else { "release" },
        timing_scope: "Monotonic operation duration after fixture reset/owner acquisition, through result checks and owner cleanup. Excludes app startup, binary hashing, fixture creation/deletion.",
        binary_sha256: None,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        kernel: fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|s|s.trim().to_owned()),
        bubblewrap_version: "unavailable".into(),
        gate_status: "passed".into(),
        public_dependencies,
        dependency_projects,
        artifact_origin: if artifact_directory.is_some() { "Externally supplied files, size/SHA-256 verified and seeded into disposable fixture caches. Broker cache reuse only; not a broker DNS/TLS/download success." } else if public_dependencies { "Production broker direct public HTTPS preparation; failures remain failures." } else { "Synthetic local fixture only; no public artifacts." },
        public_pins: if public_dependencies {
            let mut pins: serde_json::Value = serde_json::from_str(PUBLIC_PINS)?;
            pins.as_object_mut().unwrap().retain(|key,_| if dependency_projects { key=="npm_project" || key=="python_hook" } else { key=="node" || key=="precommit" });
            pins
        } else { serde_json::Value::Null },
        warm_samples: warm,
        cold_samples: cold,
        requested_counts_meet_recommendation: warm>=30 && cold>=5,
        recommended_sample_count_met: false,
        fixture_sha256: digest(&serde_json::to_vec(&public)?),
        fixture_public_files: public.len(),
        fixture_public_bytes: public.values().map(String::len).sum(),
        cache_definitions: BTreeMap::from([("cold","Fresh Greenlight project/state; fixture creation excluded from elapsed time. OS/page/toolchain/registry caches are not flushed."),("warm","Same synthetic project/state; check cache/venv remains fresh per invocation. Local pinned fixture objects only; no public downloads.")]),
        unmeasured: vec!["first-output latency for shell cases without retained execution measurements","per-phase elapsed time for non-verification shell/inspection operations","aggregate concurrent process-tree peak RSS and bytes written; cache footprint is retained regular-file metadata only, resource quotas are separate","TUI stop acknowledgement (stop-to-cleanup is measured separately)"],
        remaining_validation: vec!["Supported-host successful results must be reviewed before GL-035 closes","Real public HTTPS/TLS/DNS pinned artifact preparation","Representative remote pre-commit/Node dependency bundles","Original Git staged/history/worktree semantics remain unsupported, not tested as equivalent"],
        retained_paths: Vec::new(),
        cases: Vec::new(),

    };
    if public_dependencies {
        report.cache_definitions.insert("warm", "Same synthetic project/state; verified public artifact objects reused. Installation, disposable check workspace/cache/venv recreated for every sample. Fetch verification and offline setup included in elapsed time.");
        report.cache_definitions.insert("cold", "Fresh project/state and artifact cache; actual public downloads, hash verification and offline setup included. OS/page/network caches not flushed; fixture creation excluded.");
        report.remaining_validation = vec!["Review public-runtime results before GL-035 closes", "Representative remote pre-commit environments remain untested (local/system hook only)", "Original Git staged/history/worktree semantics remain unsupported, not tested as equivalent"];
    }
    if dependency_projects {
        report.remaining_validation = vec!["Review dependency-project results before GL-035 closes", "Network Git clone/registry discovery are deliberately not enabled: hook source uses a pinned archive and a reconstructed local mirror", "Original project staged/history/worktree semantics remain unsupported"];
    }
    if artifact_directory.is_some() {
        report.cache_definitions.insert("cold", "Fresh project/state, externally supplied pinned artifacts copied/verified into cache during each sample. No broker download measured; OS/page caches not flushed.");
        report.cache_definitions.insert("warm", "Same project/state; external artifact bytes reverified/copied each sample, broker cache reuse, fresh offline installation/cache/venv each invocation. No broker download measured.");
        report.remaining_validation.push(
            "Real broker public HTTPS/TLS/DNS fetch remains unvalidated in external-artifact mode",
        );
    }
    if let Ok(mut file) = fs::File::open("/proc/self/exe") {
        let mut digest = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        report.binary_sha256 = Some(format!("{:x}", digest.finalize()));
    }
    let version_fixture = Fixture::new(artifact_directory)?;
    let jobs = version_fixture
        .store
        .directory_fd
        .create_dirs(Path::new("sandbox-jobs"))?;
    let version: Result<String> = async {
        let mut command = crate::sandbox_job::Job::new("/usr/bin/bwrap", &jobs)?;
        command.arg("--version").env_clear().current_dir("/");
        let output = crate::sandbox::pipeline::run(
            vec![command],
            vec!["bubblewrap version".into()],
            vec![],
            &Arc::new(AtomicBool::new(false)),
            Duration::from_secs(3),
        )
        .await?;
        if !output.output.status.success() {
            bail!(
                "Version probe {}: {}",
                output.output.status,
                String::from_utf8_lossy(&output.output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.output.stdout)
            .trim()
            .to_owned())
    }
    .await;
    report.bubblewrap_version = version.unwrap_or_else(|e| format!("Unavailable: {e:#}"));
    if crate::sandbox_job::ensure_clean(&version_fixture.store.directory_fd).is_err() {
        report.gate_status = "failed".into();
        report.retained_paths = version_fixture.owned_paths();
        report.cases = scenarios.iter().map(|(scenario,name)|Case { name:(*name).into(),expectation:expectation(*scenario),status:"not_run".into(),reason:Some("Version probe cleanup unconfirmed; retained fixture on stderr; no replacement execution".into()),samples:Vec::new(),summaries:BTreeMap::new() }).collect();
        return Ok(report);
    }
    drop(version_fixture);
    let mut isolation_ready = false;
    let mut cleanup_failed = false;
    for &(scenario, name) in &scenarios {
        eprintln!("Validation case: {name}");
        if (!isolation_ready && !matches!(scenario, Scenario::Isolation)) || cleanup_failed {
            report.cases.push(Case {
                name: name.into(),
                expectation: expectation(scenario),
                status: "not_run".into(),
                reason: Some(
                    if cleanup_failed {
                        "Prior cleanup unconfirmed; no replacement execution"
                    } else {
                        "Isolation preflight did not pass; no host fallback"
                    }
                    .into(),
                ),
                samples: Vec::new(),
                summaries: BTreeMap::new(),
            });
            continue;
        }
        let mut samples = Vec::new();
        for index in 1..=cold {
            let mut fixture = Fixture::new(artifact_directory)?;
            let row = sample(&mut fixture, scenario, "cold", index, keep_failures).await?;
            cleanup_failed |= !row.cleanup_confirmed;
            samples.push(row);
            if cleanup_failed {
                break;
            }
        }
        if !cleanup_failed {
            let mut fixture = Fixture::new(artifact_directory)?;
            let warmup = sample(&mut fixture, scenario, "warmup", 0, keep_failures).await?;
            cleanup_failed |= !warmup.cleanup_confirmed;
            samples.push(warmup);
            for index in 1..=warm {
                if cleanup_failed {
                    break;
                }
                let row = sample(&mut fixture, scenario, "warm", index, keep_failures).await?;
                cleanup_failed |= !row.cleanup_confirmed;
                samples.push(row);
                if cleanup_failed {
                    break;
                }
            }
        }
        let status = if samples.iter().any(|s| s.gate == "failed") {
            "failed"
        } else if samples.iter().any(|s| s.gate == "blocked") {
            "blocked"
        } else {
            "passed"
        };
        if matches!(scenario, Scenario::Isolation) {
            isolation_ready = status == "passed";
        }
        let summaries = BTreeMap::from([
            ("warmup".into(), summary(&samples, "warmup")),
            ("cold".into(), summary(&samples, "cold")),
            ("warm".into(), summary(&samples, "warm")),
        ]);
        report.cases.push(Case {
            name: name.into(),
            expectation: expectation(scenario),
            status: status.into(),
            reason: None,
            samples,
            summaries,
        });
    }
    report.recommended_sample_count_met = report.cases.iter().all(|case| {
        case.samples.iter().filter(|s| s.cache == "cold").count() >= 5
            && case.samples.iter().filter(|s| s.cache == "warm").count() >= 30
    });
    report.retained_paths = report
        .cases
        .iter()
        .flat_map(|case| {
            case.samples
                .iter()
                .flat_map(|sample| sample.retained_paths.iter().cloned())
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    report.gate_status = if report.cases.iter().any(|c| c.status == "failed") {
        "failed"
    } else if !isolation_ready {
        "blocked"
    } else if report.cases.iter().any(|c| c.status != "passed") {
        "incomplete"
    } else {
        "passed"
    }
    .into();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(mode: &str, gate: &str, ms: f64) -> Sample {
        Sample {
            cache: mode.into(),
            index: 1,
            elapsed_ms: ms,
            gate: gate.into(),
            observed: "fixture".into(),
            diagnostic_excerpt: String::new(),
            diagnostic_sha256: digest(b""),
            cleanup_confirmed: true,
            source_files: None,
            source_bytes: None,
            stop_to_cleanup_ms: None,
            phase_ms: BTreeMap::new(),
            executions: BTreeMap::new(),
            cache_storage: None,
            retained_paths: Vec::new(),
        }
    }
    #[test]
    fn public_recipes_use_pinned_artifacts_and_only_bootstrap_requirements() {
        for scenario in [
            Scenario::PreparedNode,
            Scenario::PreparedHook,
            Scenario::NpmProject,
            Scenario::PythonHookProject,
        ] {
            let recipe = public_recipe(scenario).unwrap();
            recipe.validate().unwrap();
            assert!(!recipe
                .requires
                .iter()
                .any(|name| name == "node" || name == "pre-commit"));
            let prep = recipe.preparation.unwrap();
            assert!(!prep.artifacts.is_empty());
            assert!(prep.run.unwrap().contains("/cache/"));
        }
    }

    #[test]
    fn npm_lock_graph_matches_the_fixed_artifact_manifest() {
        let files = files();
        let package: serde_json::Value = serde_json::from_str(&files["package.json"]).unwrap();
        let lock: serde_json::Value = serde_json::from_str(&files["package-lock.json"]).unwrap();
        let pins: BTreeMap<String, Vec<crate::check_dependencies::Artifact>> =
            serde_json::from_str(PUBLIC_PINS).unwrap();
        assert_eq!(
            package["dependencies"],
            lock["packages"][""]["dependencies"]
        );
        assert_eq!(lock["packages"].as_object().unwrap().len(), 3);
        for (path, metadata) in lock["packages"].as_object().unwrap() {
            let Some(module) = path.strip_prefix("node_modules/") else {
                continue;
            };
            let filename = format!("{module}-{}.tgz", metadata["version"].as_str().unwrap());
            let artifact = pins["npm_project"]
                .iter()
                .find(|a| a.name == filename)
                .unwrap();
            assert_eq!(metadata["resolved"].as_str().unwrap(), artifact.url);
            assert!(metadata["integrity"]
                .as_str()
                .unwrap()
                .starts_with("sha512-"));
        }
    }

    #[tokio::test]
    async fn supplied_artifacts_fail_closed_before_any_fetch_or_check() {
        use std::os::unix::fs::symlink;
        for linked in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let recipe = public_recipe(Scenario::PreparedNode).unwrap();
            let name = &recipe.preparation.as_ref().unwrap().artifacts[0].name;
            let path = directory.path().join(name);
            if linked {
                symlink("/etc/passwd", &path).unwrap();
            } else {
                fs::write(&path, "wrong bytes").unwrap();
            }
            let mut fixture = Fixture::new(Some(directory.path())).unwrap();
            let result = sample(&mut fixture, Scenario::PreparedNode, "cold", 1, false)
                .await
                .unwrap();
            assert_eq!(result.gate, "failed");
            assert!(result.cleanup_confirmed);
            assert!(!fixture
                .store
                .history(&fixture.session)
                .unwrap()
                .iter()
                .any(|event| event.2.starts_with("dependency.")
                    || event.2.starts_with("verification.")));
        }
    }

    #[test]
    fn unconfirmed_jobs_retain_both_source_and_external_sentinels() {
        use std::os::unix::fs::FileExt;
        let fixture = Fixture::new(None).unwrap();
        let paths = fixture.owned_paths();
        let jobs = fixture
            .store
            .directory_fd
            .create_dirs(Path::new("sandbox-jobs"))
            .unwrap();
        let file = jobs.lock_file(Path::new("unfinished.lease")).unwrap();
        file.write_all_at(b"pending ", 0).unwrap();
        drop(fixture);
        for path in &paths {
            assert!(Path::new(path).is_dir());
        }
        // No child exists in this fixture; the pending marker is injected only
        // to prove that the harness never garbage-collects uncertain ownership.
        for path in paths {
            fs::remove_dir_all(path).unwrap();
        }
    }
    #[test]
    fn percentiles_keep_blocked_failed_and_warmup_results_separate() {
        let mut samples = vec![
            row("warm", "passed", 1.0),
            row("warm", "blocked", 5.0),
            row("warm", "failed", 100.0),
            row("warmup", "passed", 500.0),
            row("cold", "passed", 700.0),
        ];
        samples[0].executions.insert(
            "check".into(),
            crate::sandbox::pipeline::Measurements {
                first_stdout_ms: Some(0.1),
                first_stderr_ms: None,
                elapsed_ms: 0.5,
                stage_resources: vec![Some(crate::sandbox_job::ChildUsage {
                    user_cpu_us: 1,
                    system_cpu_us: 2,
                    largest_child_maxrss_kib: 3,
                    input_block_operations: 4,
                    output_block_operations: 5,
                })],
            },
        );
        samples[1].executions.insert(
            "check".into(),
            crate::sandbox::pipeline::Measurements {
                first_stdout_ms: None,
                first_stderr_ms: Some(0.5),
                elapsed_ms: 2.0,
                stage_resources: vec![None],
            },
        );
        samples[0].cache_storage = Some(crate::verification_storage::Observation {
            inspection_ms: 1.0,
            footprint: Some(crate::verification_storage::Footprint {
                entries: 1,
                unique_regular_files: 1,
                logical_regular_file_bytes: 4,
                allocated_regular_file_bytes: 0,
                skipped_symlinks: 0,
                skipped_special_files: 0,
            }),
            unavailable_reason: None,
        });
        samples[1].cache_storage = Some(crate::verification_storage::Observation {
            inspection_ms: 2.0,
            footprint: None,
            unavailable_reason: Some("budget reached".into()),
        });
        samples[0].phase_ms.insert("preflight".into(), 1.0);
        samples[1].phase_ms.insert("preflight".into(), 5.0);
        let report = summary(&samples, "warm");
        assert_eq!(report.phase_ms["preflight"].measured_samples, 2);
        assert_eq!(
            report.execution_statistics["cache.logical_regular_file_bytes"].measured_samples,
            1
        );
        assert_eq!(
            report.execution_statistics["cache.logical_regular_file_bytes"].p50,
            4.0
        );
        assert_eq!(
            report.execution_statistics["cache.allocated_regular_file_bytes"].p50,
            0.0
        );
        assert_eq!(
            report.execution_statistics["check.first_stdout"].measured_samples,
            1
        );
        assert_eq!(report.execution_statistics["check.first_stdout"].p50, 0.1);
        assert_eq!(
            report.execution_statistics["check.stage_0.largest_child_maxrss"].measured_samples,
            1
        );
        assert_eq!(
            report.execution_statistics["check.stage_0.largest_child_maxrss"].unit,
            "KiB"
        );
        assert_eq!(report.phase_ms["preflight"].p95_ms, 5.0);
        assert!(!report.phase_ms.contains_key("check_execution"));
        assert_eq!(report.samples, 3);
        assert_eq!(report.met_expectations, 1);
        assert_eq!(report.blocked, 1);
        assert_eq!(report.failed, 1);
        assert_eq!(report.all_p50_ms, Some(5.0));
        assert_eq!(report.all_p95_ms, Some(100.0));
        assert_eq!(report.met_expectations_p95_ms, Some(1.0));
        let blocked = summary(&[row("warm", "blocked", 10.0)], "warm");
        assert!(blocked.met_expectations_p50_ms.is_none());
        assert!(summary(&[], "warm").all_p95_ms.is_none());
    }
    #[test]
    fn fixtures_have_no_real_keys_and_reset_only_owned_source() {
        let fixture = Fixture::new(None).unwrap();
        assert!(fixture.path().starts_with("/tmp"));
        assert!(fixture.source_matches(&[]).unwrap());
        fs::write(fixture.path().join("data.txt"), "formatted\n").unwrap();
        assert!(!fixture.source_matches(&[]).unwrap());
        assert!(fixture
            .source_matches(&[("data.txt", "formatted\n")])
            .unwrap());
        fixture.reset().unwrap();
        assert!(fixture.source_matches(&[]).unwrap());
        let plan = crate::project_inspection::parse(fixture.path(), "ls -a").unwrap();
        let view = plan.snapshot(fixture.path()).unwrap();
        for path in [".env", ".git", ".greenlight"] {
            assert!(!view.path().join(path).exists());
        }
    }
    #[test]
    fn argument_quoting_preserves_paths_and_multiline_scripts() {
        assert_eq!(quote("a'b"), "'a'\\''b'");
        assert_eq!(quote("a\nb"), "'a\nb'");
        assert!(percentile(&[], 0.5).is_none());
        assert_eq!(percentile(&[3.0, 1.0, 2.0], 0.5), Some(2.0));
    }
    #[tokio::test]
    async fn report_preserves_blocked_setup_and_never_calls_it_a_success() {
        let mut fixture = Fixture::new(None).unwrap();
        let row = sample(&mut fixture, Scenario::Isolation, "cold", 1, false)
            .await
            .unwrap();
        assert!(row.cleanup_confirmed, "{}", row.diagnostic_excerpt);
        assert!(
            matches!(row.gate.as_str(), "passed" | "blocked"),
            "{}",
            row.diagnostic_excerpt
        );
        if row.gate == "blocked" {
            assert!(summary(&[row], "cold").met_expectations_p50_ms.is_none());
        }
        assert!(fixture.store.project_record().unwrap().is_none());
        assert_eq!(
            fixture
                .store
                .usage_report(&fixture.session, crate::usage::Scope::Session)
                .unwrap()
                .totals
                .requests,
            0
        );
    }
    #[tokio::test]
    #[ignore = "requires working Linux user namespaces, all declared runtimes and Python venv"]
    async fn integration_all_synthetic_validation_gates() {
        let report = run(1, 1, false, false, false, None).await.unwrap();
        assert_eq!(
            report.gate_status,
            "passed",
            "{}",
            serde_json::to_string_pretty(&report).unwrap()
        );
        assert!(!report.recommended_sample_count_met);
        assert!(report
            .cases
            .iter()
            .all(|case| case.samples.iter().all(|sample| sample.cleanup_confirmed)));
    }
}
