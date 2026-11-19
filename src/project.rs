//! Project-local setup from embedded resources; never depends on an install checkout.
use anyhow::{bail, Context, Result};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

#[path = "project_templates.rs"]
mod templates;

pub fn resolve_root(explicit: Option<&Path>, cwd: &Path, initializing: bool) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return path.canonicalize().context("invalid --root");
    }
    let current = cwd.canonicalize()?;
    if initializing {
        return Ok(current);
    }
    for directory in current.ancestors() {
        if [
            ".greenlight",
            ".stepwright",
            "greenlight.yaml",
            "stepwright.yaml",
        ]
        .iter()
        .any(|name| fs::symlink_metadata(directory.join(name)).is_ok())
        {
            return Ok(directory.to_owned());
        }
        // Do not accidentally attach an unrelated nested repository to its parent.
        if fs::symlink_metadata(directory.join(".git")).is_ok() {
            break;
        }
    }
    Ok(current)
}

pub fn load_environment(root: &Path) -> Result<()> {
    crate::paths::existing_directory(&root.join(crate::paths::PROJECT_DIR))?;
    crate::config::load_dotenv(&root.join(".greenlight/.env"))?;
    crate::config::load_dotenv(&root.join(".env"))
}

fn ensure_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        Ok(_) => bail!(
            "Project control directory is not a real directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn create_file(control: &Path, relative: &str, text: &str) -> Result<bool> {
    let mut path = control.to_owned();
    let parts = Path::new(relative).components().collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            bail!("Invalid embedded project path");
        };
        path.push(name);
        if index + 1 < parts.len() {
            ensure_directory(&path)?;
        }
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(&path)?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                bail!("Refusing unsafe existing project file: {}", path.display());
            }
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

pub fn init(root: &Path) -> Result<String> {
    let control = root.join(".greenlight");
    ensure_directory(&control)?;
    let legacy_config = ["greenlight.yaml", "stepwright.yaml"]
        .iter()
        .any(|p| fs::symlink_metadata(root.join(p)).is_ok());
    let local_config = fs::symlink_metadata(control.join("config.yaml")).is_ok();
    if local_config || legacy_config {
        crate::config::load_config(root)?;
    }
    let mut created = 0;
    if !legacy_config || local_config {
        created += usize::from(create_file(
            &control,
            "config.yaml",
            include_str!("../templates/config.yaml"),
        )?);
    }
    if control.join("instructions.md").exists()
        || !root.join(".stepwright/instructions.md").exists()
    {
        created += usize::from(create_file(
            &control,
            "instructions.md",
            include_str!("../templates/instructions.md"),
        )?);
    }
    created += usize::from(create_file(
        &control,
        ".env.example",
        include_str!("../templates/env.example"),
    )?);
    created += usize::from(create_file(
        &control,
        ".gitignore",
        "/state/\n/.env\n/.env.*\n!/.env.example\n",
    )?);
    let keep_legacy_workflows = legacy_config && root.join("workflows").is_dir();
    if !keep_legacy_workflows {
        for (relative, body) in templates::FILES {
            created += usize::from(create_file(&control, relative, body)?);
        }
    }
    crate::config::load_config(root)?;
    crate::paths::state_directory(root)?;
    Ok(format!("Initialized Greenlight in {} ({created} new files; existing files preserved).\nConfiguration: {}\nWorkflows: {}\nSet your API key in the environment or .greenlight/.env (see .env.example), then run `greenlight` or `greenlight tui`.{}",control.display(),crate::paths::config_path(root)?.display(),crate::paths::workflows_directory(root)?.display(),if legacy_config {"\nExisting root-level configuration was preserved. You can move it to .greenlight/config.yaml when ready."} else {""}))
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
