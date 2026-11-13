//! Linux shell sandbox: no network or inherited credentials, a read-only
//! project checkout, and a dedicated writable build cache.
use anyhow::{bail, Context, Result};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};
use tokio::process::Command;
use walkdir::WalkDir;

const BWRAP: &str = "/usr/bin/bwrap";
const WORKSPACE: &str = "/workspace";
const CACHE: &str = "/cache";

pub struct Sandbox {
    root: PathBuf,
    cache: PathBuf,
}

impl Sandbox {
    pub fn open(root: &Path) -> Result<Self> {
        #[cfg(not(target_os = "linux"))]
        bail!("shell execution requires Linux and bubblewrap; no unsandboxed fallback");

        let root = root.canonicalize().context("sandbox project root does not exist")?;
        // Reject roots that would expose unrelated host directories.
        if root == Path::new("/") {
            bail!("refusing to sandbox the entire host filesystem as a project");
        }
        if let Some(home) = env::var_os("HOME") {
            let home = PathBuf::from(home);
            if home.is_absolute() && home.starts_with(&root) {
                bail!("project root contains the host HOME; select a narrower project");
            }
        }
        // Check each component before creating the next, to prevent a symlink
        // under the project from redirecting the writable sandbox cache.
        let state = crate::paths::state_directory(&root)?;
        checked_directory(&state)?;
        let cache = state.join("sandbox-cache");
        checked_directory(&cache)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cache, fs::Permissions::from_mode(0o700))?;
        }

        Ok(Self { root, cache })
    }

    fn build(&self, input: &str) -> Result<Command> {
        if !Path::new(BWRAP).is_file() {
            bail!(
                "bubblewrap ({BWRAP}) is required for shell commands; install the 'bubblewrap' package. No host-shell fallback."
            );
        }

        let mut command = Command::new(BWRAP);
        command.kill_on_drop(true);
        command.env_clear();
        command.args([
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-net",
            "--unshare-uts",
            "--die-with-parent",
            "--new-session",
            "--hostname", "greenlight",
            "--proc", "/proc",
            "--dev", "/dev",
            "--tmpfs", "/tmp",
            "--tmpfs", "/run",
            "--tmpfs", "/home",
            "--dir", "/home/agent",
            "--dir", "/etc",
            "--dir", WORKSPACE,
            "--dir", CACHE,
        ]);

        // Only explicitly selected host system locations are visible. The
        // project is read-only: all code changes go through native writes.
        for path in ["/usr", "/bin", "/sbin", "/lib", "/lib64", "/usr/local"] {
            if Path::new(path).is_dir() {
                command.args(["--ro-bind", path, path]);
            }
        }
        for name in ["ld.so.cache", "nsswitch.conf", "passwd", "group", "localtime"] {
            let source = Path::new("/etc").join(name);
            if source.is_file() {
                command.arg("--ro-bind").arg(&source).arg(&source);
            }
        }

        command.arg("--ro-bind").arg(&self.root).arg(WORKSPACE);
        command.arg("--bind").arg(&self.cache).arg(CACHE);

        // Mask known secret and internal paths that would otherwise be visible
        // through the read-only project bind mount. Source code itself remains
        // readable; the mask is not a general secrets scanner.
        for (relative, is_dir) in masked_paths(&self.root)? {
            let dest = Path::new(WORKSPACE).join(relative);
            if is_dir {
                command.arg("--tmpfs").arg(dest);
            } else {
                command.arg("--ro-bind").arg("/dev/null").arg(dest);
            }
        }

        // Rust development tools commonly live in HOME. Expose only the
        // tool binaries, toolchains, and offline registry cache, never all of
        // HOME or the Cargo credentials file.
        let mut has_rustup = false;
        let mut has_cargo_bin = false;
        if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
            let cargo_bin = home.join(".cargo/bin");
            if cargo_bin.is_dir() {
                command.arg("--ro-bind").arg(&cargo_bin).arg("/cargo-bin");
                has_cargo_bin = true;
            }
            let rustup = home.join(".rustup");
            if rustup.is_dir() {
                command.arg("--ro-bind").arg(&rustup).arg("/rustup");
                has_rustup = true;
            }
            let registry = home.join(".cargo/registry");
            if registry.is_dir() {
                command.args(["--dir", "/cache/cargo-home"]);
                command.arg("--ro-bind").arg(&registry)
                    .arg("/cache/cargo-home/registry");
            }
        }

        command.args(["--chdir", WORKSPACE, "--clearenv"]);
        let path = if has_cargo_bin {
            "/cargo-bin:/usr/local/bin:/usr/bin:/bin"
        } else {
            "/usr/local/bin:/usr/bin:/bin"
        };
        for (name, value) in [
            ("PATH", path),
            ("HOME", "/home/agent"),
            ("TMPDIR", "/tmp"),
            ("XDG_CACHE_HOME", "/cache/xdg"),
            ("PYTHONDONTWRITEBYTECODE", "1"),
            ("CARGO_HOME", "/cache/cargo-home"),
            ("CARGO_TARGET_DIR", "/cache/cargo-target"),
            ("CARGO_NET_OFFLINE", "true"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ] {
            command.args(["--setenv", name, value]);
        }
        if has_rustup {
            command.args(["--setenv", "RUSTUP_HOME", "/rustup"]);
        }
        command.arg("--").arg("/bin/sh").arg("-c").arg(input);
        Ok(command)
    }

    pub async fn run(&self, input: &str) -> Result<Output> {
        let mut process = self.build(input)?;
        let output = tokio::time::timeout(Duration::from_secs(120), process.output())
            .await
            .context("sandboxed command timed out after 120s")?
            .context("failed to start bubblewrap sandbox")?;

        // A namespace setup failure is not an ordinary command failure.
        // Never retry outside the sandbox.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() && stderr.trim_start().starts_with("bwrap:") {
            bail!("bubblewrap could not establish isolation (check user namespaces): {stderr}");
        }
        Ok(output)
    }
}

fn checked_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        Ok(_) => bail!("sandbox cache path is not a real directory: {}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).with_context(|| format!("creating {}", path.display()))
        }
        Err(error) => Err(error).with_context(|| format!("checking {}", path.display())),
    }
}

fn is_sensitive(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".ssh" | ".aws" | ".gnupg" | ".kube" | ".docker"
            | ".config" | ".netrc" | ".npmrc" | ".pypirc"
            | ".git-credentials" | "credentials.json" | "credentials.toml"
            | "secrets.json"
    ) || name == ".env"
        || name.starts_with(".env.")
}

fn masked_paths(root: &Path) -> Result<Vec<(PathBuf, bool)>> {
    let mut masked = Vec::new();
    let mut walker = WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry) = walker.next() {
        let entry = entry.with_context(|| format!("scanning project {}", root.display()))?;
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let parent_is_internal = entry
            .path()
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|parent| {
                parent == crate::paths::PROJECT_DIR || parent == crate::paths::LEGACY_PROJECT_DIR
            });
        let internal_state = name == "state" && parent_is_internal;
        if is_sensitive(&name) || internal_state {
            if entry.file_type().is_symlink() {
                bail!("cannot safely mask symlink at {}", entry.path().display());
            }
            let relative = entry.path().strip_prefix(root)?.to_path_buf();
            let directory = entry.file_type().is_dir();
            masked.push((relative, directory));
            if masked.len() > 1_024 {
                bail!("too many sensitive project paths to safely mask");
            }
            if directory {
                walker.skip_current_dir();
            }
        } else if entry.file_type().is_dir()
            && matches!(name.as_ref(), "node_modules" | "target" | ".venv" | "venv")
        {
            // Large generated dependency trees: still read-only, but not
            // exhaustively searched for secrets.
            walker.skip_current_dir();
        }
    }
    Ok(masked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_secret_files_and_internal_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join(".greenlight/state")).unwrap();
        fs::create_dir_all(root.join(".stepwright/state")).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join(".env"), "SECRET=hidden").unwrap();
        fs::write(root.join("src/.env.prod"), "SECRET=nested").unwrap();
        fs::write(root.join("src/lib.rs"), "fn main() {}").unwrap();
        let masks = masked_paths(root).unwrap();
        let names: Vec<_> = masks.iter()
            .map(|(path, _)| path.to_string_lossy().into_owned()).collect();
        assert!(names.contains(&".git".to_string()));
        assert!(names.contains(&".env".to_string()));
        assert!(names.contains(&"src/.env.prod".to_string()));
        assert!(names.contains(&".greenlight/state".to_string()));
        assert!(names.contains(&".stepwright/state".to_string()));
        assert!(!names.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn sandbox_arguments_require_read_only_workspace_and_network_isolation() {
        if !Path::new(BWRAP).is_file() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::open(temp.path()).unwrap();
        let command = sandbox.build("pwd").unwrap();
        let args: Vec<_> = command.as_std().get_args()
            .map(|value| value.to_string_lossy().into_owned()).collect();
        assert!(args.contains(&"--unshare-user".into()));
        assert!(args.contains(&"--unshare-net".into()));
        assert!(args.contains(&"--new-session".into()));
        assert!(args.windows(3).any(|w| {
            w[0] == "--ro-bind" && w[2] == WORKSPACE
        }));
        assert!(!args.windows(3).any(|w| {
            w[0] == "--bind" && w[2] == WORKSPACE
        }));
        assert!(args.contains(&"--clearenv".into()));
    }

    #[test]
    fn rejects_symlinked_state_directory() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join(".greenlight")).unwrap();
            symlink(outside.path(), root.path().join(".greenlight/state")).unwrap();
            assert!(Sandbox::open(root.path()).is_err());
        }
    }

    #[tokio::test]
    #[ignore = "requires installed bubblewrap and enabled Linux user namespaces"]
    async fn integration_restricts_host_access() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join(".env"), "SECRET=hidden").unwrap();
        fs::write(root.join("main.py"), "print('ok')").unwrap();
        let sandbox = Sandbox::open(root).unwrap();
        let output = sandbox.run(
            "test ! -w /workspace && test -w /tmp && test -w /cache && \
             test ! -s /workspace/.env && test ! -e /workspace/.greenlight/state/history.db && \
             test ! -e /home/agent/.ssh && test -z \"${DEEPINFRA_API_KEY:-}\" && printf 'ok\\n'"
        ).await.unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
}
