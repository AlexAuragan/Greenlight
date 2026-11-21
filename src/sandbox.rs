//! Linux shell sandbox: no network or inherited credentials, a read-only
//! project checkout, and a dedicated writable build cache.
use anyhow::{bail, Context, Result};
#[cfg(test)]
use std::fs;
use std::{
    env,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};
use tokio::process::Command;

const BWRAP: &str = "/usr/bin/bwrap";
const WORKSPACE: &str = "/workspace";
const CACHE: &str = "/cache";

pub struct Sandbox {
    root_fd: crate::anchored_fs::Dir,
    cache_fd: crate::anchored_fs::Dir,
}

impl Sandbox {
    pub fn open(root: &Path) -> Result<Self> {
        #[cfg(not(target_os = "linux"))]
        bail!("shell execution requires Linux and bubblewrap; no unsandboxed fallback");

        let root = root
            .canonicalize()
            .context("sandbox project root does not exist")?;
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
        let (_, state) = crate::paths::state_directory_handle(&root)?;
        let cache_fd = state.create_dirs(Path::new("sandbox-cache"))?;
        use std::os::unix::fs::PermissionsExt;
        cache_fd
            .clone_file()?
            .set_permissions(std::fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            root_fd: crate::anchored_fs::Dir::open(&root)?,
            cache_fd,
        })
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
            "--hostname",
            "greenlight",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--tmpfs",
            "/run",
            "--tmpfs",
            "/home",
            "--dir",
            "/home/agent",
            "--dir",
            "/etc",
            "--dir",
            WORKSPACE,
            "--dir",
            CACHE,
        ]);

        // Only explicitly selected host system locations are visible. The
        // project is read-only: all code changes go through native writes.
        for path in ["/usr", "/bin", "/sbin", "/lib", "/lib64", "/usr/local"] {
            if Path::new(path).is_dir() {
                command.args(["--ro-bind", path, path]);
            }
        }
        for name in [
            "ld.so.cache",
            "nsswitch.conf",
            "passwd",
            "group",
            "localtime",
        ] {
            let source = Path::new("/etc").join(name);
            if source.is_file() {
                command.arg("--ro-bind").arg(&source).arg(&source);
            }
        }

        use std::os::fd::AsRawFd;
        let root = self.root_fd.clone_file()?;
        let cache = self.cache_fd.clone_file()?;
        let root_fd = root.as_raw_fd();
        let cache_fd = cache.as_raw_fd();
        command.args(["--ro-bind-fd", &root_fd.to_string(), WORKSPACE]);
        command.args(["--bind-fd", &cache_fd.to_string(), CACHE]);
        // SAFETY: only async-signal-safe fcntl calls run in the child. Owned
        // clones keep these exact directories alive until command execution.
        unsafe {
            command.pre_exec(move || {
                let _keep_alive = (&root, &cache);
                for fd in [root_fd, cache_fd] {
                    if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }

        // Mask known secret and internal paths that would otherwise be visible
        // through the read-only project bind mount. Source code itself remains
        // readable; the mask is not a general secrets scanner.
        for (relative, is_dir) in masked_directories(&self.root_fd)? {
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
                command
                    .arg("--ro-bind")
                    .arg(&registry)
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
            bail!("bubblewrap could not establish isolation (check user namespaces and --bind-fd/--ro-bind-fd support): {stderr}");
        }
        Ok(output)
    }
}

fn is_sensitive(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".ssh"
            | ".aws"
            | ".gnupg"
            | ".kube"
            | ".docker"
            | ".config"
            | ".netrc"
            | ".npmrc"
            | ".pypirc"
            | ".git-credentials"
            | "credentials.json"
            | "credentials.toml"
            | "secrets.json"
    ) || name == ".env"
        || name.starts_with(".env.")
}

#[cfg(test)]
fn masked_paths(root: &Path) -> Result<Vec<(PathBuf, bool)>> {
    masked_directories(&crate::anchored_fs::Dir::open(root)?)
}
fn masked_directories(root: &crate::anchored_fs::Dir) -> Result<Vec<(PathBuf, bool)>> {
    let mut masked = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        for entry in root.open_dir(&relative)?.entries()? {
            let entry = entry?;
            let path = relative.join(&entry.name);
            let name = entry.name.to_string_lossy();
            if is_sensitive(&name)
                || crate::paths::is_private_path(&path)
                || (name == "state"
                    && path
                        .parent()
                        .and_then(Path::file_name)
                        .is_some_and(|parent| {
                            parent == crate::paths::PROJECT_DIR
                                || parent == crate::paths::LEGACY_PROJECT_DIR
                        }))
            {
                if entry.symlink {
                    bail!("cannot safely mask symlink at {}", path.display());
                }
                masked.push((path, entry.directory));
                if masked.len() > 1024 {
                    bail!("too many sensitive project paths to safely mask");
                }
            } else if entry.directory
                && !matches!(name.as_ref(), "node_modules" | "target" | ".venv" | "venv")
            {
                pending.push(path);
            }
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
        let names: Vec<_> = masks
            .iter()
            .map(|(path, _)| path.to_string_lossy().into_owned())
            .collect();
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
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--unshare-user".into()));
        assert!(args.contains(&"--unshare-net".into()));
        assert!(args.contains(&"--new-session".into()));
        assert!(args
            .windows(3)
            .any(|w| { w[0] == "--ro-bind-fd" && w[2] == WORKSPACE }));
        assert!(!args
            .windows(3)
            .any(|w| { matches!(w[0].as_str(), "--bind" | "--bind-fd") && w[2] == WORKSPACE }));
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
        crate::project::init(root).unwrap();
        fs::write(root.join(".greenlight/.env"), "PRIVATE_KEY=hidden").unwrap();
        let sandbox = Sandbox::open(root).unwrap();
        let output = sandbox.run(
            "test ! -w /workspace && test -w /tmp && test -w /cache && \
             test ! -s /workspace/.env && test ! -s /workspace/.greenlight/config.yaml && test ! -s /workspace/.greenlight/.env && test ! -e /workspace/.greenlight/state/history.db && \
             test ! -e /home/agent/.ssh && test -z \"${DEEPINFRA_API_KEY:-}\" && printf 'ok\\n'"
        ).await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    #[test]
    fn bind_sources_remain_pinned_when_project_path_is_replaced() {
        if !Path::new(BWRAP).is_file() {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::open(root.path()).unwrap();
        let inode = fs::metadata(root.path()).unwrap().ino();
        let renamed = root.path().with_extension("pinned");
        fs::rename(root.path(), &renamed).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path()).unwrap();
        let command = sandbox.build("pwd").unwrap();
        drop(sandbox);
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        let index = args.iter().position(|arg| arg == "--ro-bind-fd").unwrap();
        assert_eq!(
            fs::metadata(format!("/proc/self/fd/{}", args[index + 1]))
                .unwrap()
                .ino(),
            inode
        );
        assert!(args.iter().any(|arg| arg == "--bind-fd"));
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        fs::remove_file(root.path()).unwrap();
        fs::rename(&renamed, root.path()).unwrap();
    }
}
