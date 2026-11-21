use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(default = "base_url")]
    pub base_url: String,
    #[serde(default = "api_key_env")]
    pub api_key_env: String,
    pub model: String,
    #[serde(default = "max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub model_metadata: std::collections::BTreeMap<String, crate::usage::ModelMetadata>,
    #[serde(default = "pricing_discovery")]
    pub pricing_discovery: bool,
}
fn pricing_discovery() -> bool {
    true
}
fn base_url() -> String {
    "https://api.deepinfra.com/v1/openai".into()
}
fn api_key_env() -> String {
    "DEEPINFRA_API_KEY".into()
}
fn max_tokens() -> u32 {
    4096
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    #[serde(default = "ask")]
    pub shell: String,
    #[serde(default = "ask")]
    pub write: String,
    #[serde(default = "allow")]
    pub read: String,
    #[serde(default = "ask")]
    pub web: String,
}
fn ask() -> String {
    "ask".into()
}
fn allow() -> String {
    "allow".into()
}
impl Default for Permissions {
    fn default() -> Self {
        Self {
            shell: ask(),
            write: ask(),
            read: allow(),
            web: ask(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    /// Legacy configuration field, accepted but no longer enforced.
    #[serde(default)]
    pub max_turns_per_attempt: usize,
    #[serde(default = "default_artifact_bytes")]
    pub max_artifact_bytes: usize,
    /// Full prepared coding conversation; execution pauses before exceeding this.
    #[serde(default = "default_conversation_bytes")]
    pub max_conversation_bytes: usize,
}
fn default_conversation_bytes() -> usize {
    256_000
}
fn default_artifact_bytes() -> usize {
    100_000
}
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_turns_per_attempt: 0,
            max_artifact_bytes: default_artifact_bytes(),
            max_conversation_bytes: default_conversation_bytes(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Notifications {
    pub discord_webhook_env: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub provider: ProviderConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub notifications: Notifications,
    #[serde(default)]
    pub manager: ManagerConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerConfig {
    #[serde(default)]
    pub enabled: bool,
    pub model: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub id: String,
    pub title: String,
    pub start: String,
    pub steps: Vec<String>,
    #[serde(default)]
    pub defaults: StepDefaults,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StepDefaults {
    pub model: Option<String>,
    /// Legacy configuration field, accepted but no longer enforced.
    pub max_turns: Option<usize>,
    pub tools: Option<Vec<String>>,
    pub verification: Option<Verify>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub from: Vec<ArtifactSelector>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSelector {
    pub source: String, // previous | session | step identifier
    #[serde(default = "all")]
    pub select: String, // all | required
}
fn all() -> String {
    "all".into()
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Outputs {
    #[serde(default)]
    pub required: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Verify {
    None,
    Command { run: String },
    Agent { prompt: String },
    Human,
    All { checks: Vec<Verify> },
    Any { checks: Vec<Verify> },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Deviation {
    pub id: String,
    pub prompt: Option<String>,
    #[serde(default = "pause")]
    pub action: String, // pause | goto | retry
    pub target: Option<String>,
    pub max_retries: Option<u32>,
}
fn pause() -> String {
    "pause".into()
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub success: Option<String>, // omitted = workflow completed
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    #[serde(default = "agent")]
    pub kind: String, // agent | human | command
    pub command: Option<String>,
    pub model: Option<String>,
    /// Legacy configuration field, accepted but no longer enforced.
    pub max_turns: Option<usize>,
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub inputs: Inputs,
    #[serde(default)]
    pub outputs: Outputs,
    pub verification: Option<Verify>,
    #[serde(default)]
    pub deviations: Vec<Deviation>,
    #[serde(default)]
    pub transitions: Transition,
    #[serde(default)]
    pub prompt: String,
}
fn agent() -> String {
    "agent".into()
}

#[derive(Clone)]
pub struct LoadedWorkflow {
    pub definition: Workflow,
    pub steps: HashMap<String, Step>,
    pub folder: PathBuf,
}

pub fn load_config(root: &Path) -> Result<Config> {
    let path = crate::paths::config_path(root)?;
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let cfg: Config = serde_yaml::from_str(&text)?;
    for value in [
        &cfg.permissions.shell,
        &cfg.permissions.write,
        &cfg.permissions.read,
        &cfg.permissions.web,
    ] {
        if !["ask", "allow", "deny"].contains(&value.as_str()) {
            bail!("permission must be ask, allow or deny: {value}");
        }
    }
    if cfg.provider.model.is_empty() {
        bail!("provider model must be configured");
    }
    if cfg
        .manager
        .model
        .as_ref()
        .is_some_and(|v| v.trim().is_empty())
    {
        bail!("manager model must not be empty");
    }
    if cfg
        .provider
        .service_tier
        .as_deref()
        .is_some_and(|tier| tier != "priority")
    {
        bail!("service_tier supports priority; omit it for the provider default");
    }
    if cfg.session.max_conversation_bytes == 0 {
        bail!("max_conversation_bytes must be positive");
    }
    for metadata in cfg.provider.model_metadata.values() {
        metadata.validate()?;
    }
    Ok(cfg)
}

/// Populate process environment variables from a `.env` file, if it exists.
/// Existing variables are never overridden, so shell exports and real
/// environment settings keep precedence over the file.
pub fn load_dotenv(path: &Path) -> Result<()> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map(str::trim).unwrap_or(line);
        let (key, value) = line
            .split_once('=')
            .with_context(|| format!("{}:{}: expected KEY=VALUE", path.display(), index + 1))?;
        let key = key.trim();
        if key.is_empty() {
            bail!("{}:{}: empty key", path.display(), index + 1);
        }
        let value = value.trim();
        let value = match value.as_bytes() {
            [b'"', .., b'"'] | [b'\'', .., b'\''] => &value[1..value.len() - 1],
            _ => value,
        };
        if std::env::var_os(key).is_none() {
            std::env::set_var(key, value);
            crate::credentials::record_dotenv(key, value, path);
        }
    }
    Ok(())
}

pub fn load_workflow(root: &Path, id: &str) -> Result<LoadedWorkflow> {
    if id.contains('/') || id.contains('\\') || id == ".." || id.starts_with('.') {
        bail!("invalid workflow name");
    }
    let folder = crate::paths::workflows_directory(root)?.join(id);
    let definition: Workflow = serde_yaml::from_str(&fs::read_to_string(
        crate::paths::project_file(root, &folder.join("workflow.yaml"))?,
    )?)?;
    if definition.id != id {
        bail!("workflow id does not match directory");
    }
    let mut steps = HashMap::new();
    for name in &definition.steps {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            bail!("invalid step filename {name}");
        }
        let source = fs::read_to_string(crate::paths::project_file(
            root,
            &folder.join(format!("{name}.md")),
        )?)
        .with_context(|| format!("missing step {name}.md"))?;
        let (header, body) = frontmatter(&source)?;
        let mut step: Step = serde_yaml::from_str(header)?;
        step.prompt = body.to_string();
        if step.id != *name {
            bail!("step filename and id differ for {name}");
        }
        if step.model.is_none() {
            step.model = definition.defaults.model.clone();
        }
        if step.tools.is_none() {
            step.tools = definition.defaults.tools.clone();
        }
        if step.max_turns.is_none() {
            step.max_turns = definition.defaults.max_turns;
        }
        if step.verification.is_none() {
            step.verification = definition.defaults.verification.clone();
        }
        if step.verification.is_none() {
            bail!("step {name} missing verification policy (including explicit none)");
        }
        if !["agent", "human", "command"].contains(&step.kind.as_str()) {
            bail!("unsupported kind for step {name}");
        }
        if step.kind == "command" && step.command.is_none() {
            bail!("command step {name} missing command");
        }
        let allowed = ["shell", "read", "write", "web"];
        for tool in step.tools.as_deref().unwrap_or(&[]) {
            if !allowed.contains(&tool.as_str()) {
                bail!("unknown tool {tool} in {name}");
            }
        }
        for d in &step.deviations {
            if !["pause", "goto", "retry"].contains(&d.action.as_str()) {
                bail!("unknown deviation action {}", d.action);
            }
            if d.action == "goto" && d.target.is_none() {
                bail!("goto deviation requires target");
            }
            if d.action == "retry" && d.max_retries.is_none() {
                bail!("retry requires max_retries");
            }
        }
        for s in &step.inputs.from {
            if s.select != "all" && s.select != "required" {
                bail!("unknown selector {}", s.select);
            }
        }
        steps.insert(name.clone(), step);
    }
    if !steps.contains_key(&definition.start) {
        bail!("unknown workflow start");
    }
    if steps.len() != definition.steps.len() {
        bail!("duplicate step name");
    }
    let ids: HashSet<_> = steps.keys().collect();
    for step in steps.values() {
        if let Some(next) = &step.transitions.success {
            if !ids.contains(next) {
                bail!("step {} references unknown next step {next}", step.id);
            }
        }
        for d in &step.deviations {
            if let Some(target) = &d.target {
                if !ids.contains(target) {
                    bail!("unknown deviation target {target}");
                }
            }
        }
        for s in &step.inputs.from {
            if s.source != "previous" && s.source != "session" && !ids.contains(&s.source) {
                bail!("unknown artifact source {}", s.source);
            }
        }
    }
    Ok(LoadedWorkflow {
        definition,
        steps,
        folder,
    })
}

pub fn frontmatter(src: &str) -> Result<(&str, &str)> {
    let text = src
        .strip_prefix("---\n")
        .context("step must begin with --- frontmatter")?;
    let (header, body) = text
        .split_once("\n---\n")
        .context("missing closing frontmatter ---")?;
    Ok((header, body))
}

pub fn render_includes(text: &str, root: &Path, folder: &Path) -> Result<String> {
    // TODO(GL-005): bound expanded template bytes, not only recursion depth.
    // Intentional restricted template syntax: includes only; no template expressions.
    let mut result = text.to_owned();
    for _ in 0..12 {
        let Some(start) = result.find("{{ include \"") else {
            return Ok(result);
        };
        let at = start + "{{ include \"".len();
        let end = result[at..].find("\" }}").context("unterminated include")? + at;
        let name = &result[at..end];
        let path = crate::paths::project_file(root, &folder.join(name))
            .with_context(|| format!("missing include {name} or inaccessible project file"))?;
        let content = fs::read_to_string(&path)?;
        result.replace_range(start..end + "\" }}".len(), &content);
    }
    if result.contains("{{ include \"") {
        bail!("include recursion limit exceeded");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_frontmatter() {
        let (h, b) = frontmatter("---\nid: demo\n---\nHello").unwrap();
        assert!(h.contains("demo"));
        assert_eq!(b, "Hello");
    }
    #[test]
    fn missing_frontmatter_rejected() {
        assert!(frontmatter("hello").is_err());
    }
    #[test]
    fn strict_verification_yaml() {
        let v: Verify = serde_yaml::from_str("type: none").unwrap();
        assert!(matches!(v, Verify::None));
    }
    #[test]
    fn new_config_takes_precedence_over_legacy() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("stepwright.yaml"),
            "provider:\n  model: old-model\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: new-model\n",
        )
        .unwrap();
        assert_eq!(
            load_config(root.path()).unwrap().provider.model,
            "new-model"
        );
    }

    #[test]
    fn legacy_config_still_loads() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("stepwright.yaml"),
            "provider:\n  model: old-model\n",
        )
        .unwrap();
        assert_eq!(
            load_config(root.path()).unwrap().provider.model,
            "old-model"
        );
    }

    #[test]
    fn dotenv_loads_without_overriding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(
            &path,
            "# comment\nGREENLIGHT_DOTENV_A=1\nexport GREENLIGHT_DOTENV_B=\"two words\"\nGREENLIGHT_DOTENV_C='three'\n",
        )
        .unwrap();
        std::env::remove_var("GREENLIGHT_DOTENV_A");
        std::env::remove_var("GREENLIGHT_DOTENV_B");
        std::env::set_var("GREENLIGHT_DOTENV_C", "preset");
        load_dotenv(&path).unwrap();
        assert_eq!(std::env::var("GREENLIGHT_DOTENV_A").unwrap(), "1");
        assert_eq!(std::env::var("GREENLIGHT_DOTENV_B").unwrap(), "two words");
        assert_eq!(std::env::var("GREENLIGHT_DOTENV_C").unwrap(), "preset");
    }
    #[test]
    fn dotenv_missing_file_is_ignored() {
        assert!(load_dotenv(Path::new("/nonexistent/greenlight/.env")).is_ok());
    }
    #[test]
    fn includes_keep_private_files_out_of_model_context() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("public.md"), "public").unwrap();
        fs::write(root.path().join(".env"), "fixture secret").unwrap();
        assert_eq!(
            render_includes("{{ include \"public.md\" }}", root.path(), root.path()).unwrap(),
            "public"
        );
        assert!(render_includes("{{ include \".env\" }}", root.path(), root.path()).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("outside.md"), "outside").unwrap();
            symlink(
                outside.path().join("outside.md"),
                root.path().join("escape.md"),
            )
            .unwrap();
            symlink(root.path().join(".env"), root.path().join("alias.md")).unwrap();
            for name in ["escape.md", "alias.md"] {
                assert!(render_includes(
                    &format!("{{{{ include \"{name}\" }}}}"),
                    root.path(),
                    root.path()
                )
                .is_err());
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn workflow_step_cannot_follow_a_private_alias() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
        )
        .unwrap();
        fs::write(root.path().join(".env"), "fixture secret").unwrap();
        std::os::unix::fs::symlink(root.path().join(".env"), folder.join("work.md")).unwrap();
        assert!(load_workflow(root.path(), "demo")
            .err()
            .unwrap()
            .to_string()
            .contains("private"));
    }
}
