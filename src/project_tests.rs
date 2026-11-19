use super::*;
use crate::{config, paths, store::Store};

#[test]
fn initialized_project_is_self_contained_and_all_embedded_workflows_render() {
    let root = tempfile::tempdir().unwrap();
    init(root.path()).unwrap();
    assert!(!root.path().join("greenlight.yaml").exists());
    assert!(!root.path().join("workflows").exists());
    assert!(root.path().join(".greenlight/config.yaml").is_file());
    assert!(root.path().join(".greenlight/state").is_dir());
    assert!(!root.path().join(".greenlight/.env").exists());
    let choices = crate::session_picker::NewSession::discover(root.path()).unwrap();
    assert_eq!(choices.workflows.len(), 3);
    for choice in choices.workflows {
        assert!(choice.error.is_none(), "{:?}", choice.error);
        let wf = config::load_workflow(root.path(), &choice.id).unwrap();
        assert!(wf
            .folder
            .starts_with(root.path().join(".greenlight/workflows")));
        for step in wf.steps.values() {
            config::render_includes(&step.prompt, root.path(), &wf.folder).unwrap();
            for rule in &step.deviations {
                if let Some(prompt) = &rule.prompt {
                    config::render_includes(
                        &format!("{{{{ include \"{prompt}\" }}}}"),
                        root.path(),
                        &wf.folder,
                    )
                    .unwrap();
                }
            }
            fn verify(check: &config::Verify, root: &Path, folder: &Path) {
                match check {
                    config::Verify::Agent { prompt } => {
                        config::render_includes(
                            &format!("{{{{ include \"{prompt}\" }}}}"),
                            root,
                            folder,
                        )
                        .unwrap();
                    }
                    config::Verify::All { checks } | config::Verify::Any { checks } => {
                        for check in checks {
                            verify(check, root, folder);
                        }
                    }
                    _ => {}
                }
            }
            verify(step.verification.as_ref().unwrap(), root.path(), &wf.folder);
        }
    }
}

#[test]
fn reinitializing_preserves_configuration_custom_workflows_and_history() {
    let root = tempfile::tempdir().unwrap();
    init(root.path()).unwrap();
    let config = root.path().join(".greenlight/config.yaml");
    let custom = fs::read_to_string(&config)
        .unwrap()
        .replace("max_tokens: 4096", "max_tokens: 777");
    fs::write(&config, &custom).unwrap();
    let workflow = root
        .path()
        .join(".greenlight/workflows/fix-bug/investigate.md");
    let text = fs::read_to_string(&workflow).unwrap() + "\nCustom instructions\n";
    fs::write(&workflow, &text).unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("fix-bug", "Retain me", "investigate").unwrap();
    let before = store.history(&s).unwrap();
    init(root.path()).unwrap();
    assert_eq!(fs::read_to_string(config).unwrap(), custom);
    assert_eq!(fs::read_to_string(workflow).unwrap(), text);
    assert_eq!(before, store.history(&s).unwrap());
    assert_eq!(
        config::load_config(root.path())
            .unwrap()
            .provider
            .max_tokens,
        777
    );
}

#[test]
fn legacy_layout_remains_selected_and_new_layout_has_explicit_precedence() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("greenlight.yaml"),
        "provider:\n  model: legacy-model\n",
    )
    .unwrap();
    fs::create_dir(root.path().join("workflows")).unwrap();
    fs::create_dir(root.path().join(".stepwright")).unwrap();
    fs::write(
        root.path().join(".stepwright/instructions.md"),
        "Keep my existing instructions",
    )
    .unwrap();
    init(root.path()).unwrap();
    assert!(!root.path().join(".greenlight/config.yaml").exists());
    assert!(!root.path().join(".greenlight/workflows").exists());
    assert!(!root.path().join(".greenlight/instructions.md").exists());
    assert_eq!(
        config::load_config(root.path()).unwrap().provider.model,
        "legacy-model"
    );
    assert_eq!(
        paths::workflows_directory(root.path()).unwrap(),
        root.path().join("workflows")
    );
    fs::write(
        root.path().join(".greenlight/config.yaml"),
        "provider:\n  model: local-model\n",
    )
    .unwrap();
    assert_eq!(
        config::load_config(root.path()).unwrap().provider.model,
        "local-model"
    );
    fs::write(root.path().join(".greenlight/config.yaml"), "invalid").unwrap();
    assert!(config::load_config(root.path()).is_err());
}

#[test]
fn discovery_is_project_scoped_explicit_root_and_nested_git_boundaries_work() {
    let root = tempfile::tempdir().unwrap();
    init(root.path()).unwrap();
    let sub = root.path().join("src/nested");
    fs::create_dir_all(&sub).unwrap();
    assert_eq!(resolve_root(None, &sub, false).unwrap(), root.path());
    assert_eq!(resolve_root(None, &sub, true).unwrap(), sub);
    assert_eq!(resolve_root(Some(&sub), root.path(), false).unwrap(), sub);
    fs::create_dir(sub.join(".git")).unwrap();
    assert_eq!(resolve_root(None, &sub, false).unwrap(), sub);
    init(&sub).unwrap();
    assert_eq!(resolve_root(None, &sub, false).unwrap(), sub);
    assert_ne!(
        Store::open(&sub).unwrap().directory,
        Store::open(root.path()).unwrap().directory
    );
}

#[test]
fn project_dotenv_precedence_and_private_paths_are_consistent() {
    let root = tempfile::tempdir().unwrap();
    init(root.path()).unwrap();
    let key = format!("GREENLIGHT_ENV_TEST_{}", uuid::Uuid::new_v4().simple());
    fs::write(
        root.path().join(".greenlight/.env"),
        format!("{key}=local\n"),
    )
    .unwrap();
    fs::write(root.path().join(".env"), format!("{key}=legacy\n")).unwrap();
    load_environment(root.path()).unwrap();
    assert_eq!(std::env::var(&key).unwrap(), "local");
    std::env::set_var(&key, "exported");
    load_environment(root.path()).unwrap();
    assert_eq!(std::env::var(&key).unwrap(), "exported");
    std::env::remove_var(key);
    for file in [
        ".greenlight/config.yaml",
        ".greenlight/.env",
        "greenlight.yaml",
        "stepwright.yaml",
        "src/.env.production",
    ] {
        assert!(paths::is_private_path(Path::new(file)));
        assert!(crate::restore::checked_path(root.path(), file).is_err());
    }
    assert!(!paths::is_private_path(Path::new(
        ".greenlight/instructions.md"
    )));
}

#[cfg(unix)]
#[test]
fn initializer_refuses_symlinked_control_directories_and_templates() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    symlink(external.path(), root.path().join(".greenlight")).unwrap();
    assert!(init(root.path()).is_err());
    assert!(fs::read_dir(external.path()).unwrap().next().is_none());
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".greenlight")).unwrap();
    symlink(external.path(), root.path().join(".greenlight/workflows")).unwrap();
    assert!(init(root.path()).is_err());
    assert!(fs::read_dir(external.path()).unwrap().next().is_none());
}
