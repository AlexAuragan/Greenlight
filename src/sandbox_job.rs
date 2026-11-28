//! Invocation control and durable cleanup evidence, outside the sandbox mounts.
use anyhow::{bail, Context, Result};
use std::{
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{FileExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::OnceLock,
};
use tokio::process::Command;

#[derive(Debug)]
pub(crate) struct CleanupIncomplete;
impl std::fmt::Display for CleanupIncomplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sandbox cleanup incomplete or unconfirmed. Project execution remains quarantined; inspect sandbox-jobs before recovery. No automatic replay.")
    }
}
impl std::error::Error for CleanupIncomplete {}

pub(super) struct Job {
    pub command: Command,
    guard: Guard,
}
pub(super) struct Guard {
    control: Option<std::io::PipeWriter>,
    lease: File,
    spawned: bool,
    // Fixtures also exercise the actual supervisor but never expose a host
    // execution option in the production helper.
    _temporary: Option<tempfile::TempDir>,
}
impl std::ops::Deref for Job {
    type Target = Command;
    fn deref(&self) -> &Command {
        &self.command
    }
}
impl std::ops::DerefMut for Job {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.command
    }
}
fn helper() -> Result<PathBuf> {
    static HELPER: OnceLock<File> = OnceLock::new();
    if HELPER.get().is_none() {
        // An anonymous, sealed executable avoids a separately installed binary,
        // temporary-file replacement races and leaked helper directories.
        let fd = unsafe {
            libc::memfd_create(
                c"greenlight-sandbox-owner".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("Create sandbox supervisor executable");
        }
        let file = unsafe { File::from_raw_fd(fd) };
        #[cfg(not(test))]
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/sandbox-supervisor")).as_slice();
        #[cfg(test)]
        let bytes =
            include_bytes!(concat!(env!("OUT_DIR"), "/sandbox-supervisor-fixture")).as_slice();
        file.write_all_at(bytes, 0)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        if unsafe {
            libc::fcntl(
                fd,
                libc::F_ADD_SEALS,
                libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL,
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error())
                .context("Seal sandbox supervisor executable");
        }
        let _ = HELPER.set(file);
    }
    Ok(PathBuf::from(format!(
        "/proc/self/fd/{}",
        HELPER.get().unwrap().as_raw_fd()
    )))
}

impl Job {
    pub fn new(program: &str, jobs: &crate::anchored_fs::Dir) -> Result<Self> {
        let executable = helper()?;
        let (read, write) = std::io::pipe()?;
        let lease = jobs.lock_file(Path::new(&format!("{}.lease", uuid::Uuid::new_v4())))?;
        lease.try_lock()?;
        let mut guard = Guard {
            control: Some(write),
            lease,
            spawned: false,
            _temporary: None,
        };
        let lease = &mut guard.lease;
        lease.write_all_at(b"pending ", 0)?;
        lease.sync_data()?;
        jobs.sync()?;
        let mut command = Command::new(executable);
        // Terminal Ctrl+C must not kill the invocation owner together with
        // Greenlight. Parent exit closes control; the owner then reaps its tree.
        command.process_group(0);
        let child_lease = lease.try_clone()?;
        command
            .arg(read.as_raw_fd().to_string())
            .arg(child_lease.as_raw_fd().to_string())
            .arg(program);
        unsafe {
            command.pre_exec(move || {
                for fd in [read.as_raw_fd(), child_lease.as_raw_fd()] {
                    if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        Ok(Self { command, guard })
    }
    #[cfg(test)]
    pub fn fixture(program: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let jobs = crate::anchored_fs::Dir::open(dir.path()).unwrap();
        let mut job = Self::new(program, &jobs).unwrap();
        job.guard._temporary = Some(dir);
        job
    }
    pub fn spawn(mut self) -> Result<(tokio::process::Child, Guard)> {
        match self.command.spawn() {
            Ok(child) => {
                self.guard.spawned = true;
                Ok((child, self.guard))
            }
            Err(error) => {
                self.guard.lease.write_all_at(b"complete", 0)?;
                Err(error.into())
            }
        }
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChildUsage {
    pub user_cpu_us: u64,
    pub system_cpu_us: u64,
    pub largest_child_maxrss_kib: u64,
    pub input_block_operations: u64,
    pub output_block_operations: u64,
}
impl Guard {
    pub fn resources(&self) -> Option<ChildUsage> {
        self.confirmed().ok()?;
        let mut buffer = [0; 256];
        let count = self.lease.read_at(&mut buffer, 256).ok()?;
        let text = std::str::from_utf8(&buffer[..count]).ok()?;
        if count == buffer.len() || !text.ends_with('\n') {
            return None;
        }
        let fields = text
            .strip_prefix("usage_v1 ")?
            .split_whitespace()
            .map(str::parse::<u64>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .ok()?;
        if fields.len() != 5 {
            return None;
        }
        Some(ChildUsage {
            user_cpu_us: fields[0],
            system_cpu_us: fields[1],
            largest_child_maxrss_kib: fields[2],
            input_block_operations: fields[3],
            output_block_operations: fields[4],
        })
    }

    pub fn cancel(&mut self) {
        self.control.take();
    }
    pub fn confirmed(&self) -> Result<()> {
        let mut state = [0; 8];
        if self.lease.read_at(&mut state, 0)? != 8 || &state != b"complete" {
            return Err(CleanupIncomplete.into());
        }
        Ok(())
    }
}
/// A dead supervisor is not proof of cleanup: only its explicit ECHILD record
/// permits recovery. An active file lock catches startup and ongoing cleanup.
pub(crate) fn ensure_clean(state: &crate::anchored_fs::Dir) -> Result<()> {
    let jobs = state.create_dirs(Path::new("sandbox-jobs"))?;
    for entry in jobs.entries()? {
        let name = entry?.name;
        let file = jobs.lock_file(Path::new(&name))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Err(CleanupIncomplete.into()),
            Err(error) => return Err(error.into()),
        }
        let mut status = [0; 8];
        if file.read_at(&mut status, 0)? != 8 || &status != b"complete" {
            bail!(CleanupIncomplete);
        }
        jobs.target(Path::new(&name))?.unlink()?;
    }
    Ok(())
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.spawned {
            let _ = self.lease.write_all_at(b"complete", 0);
        }
        if self.confirmed().is_ok() {
            let _ = self.lease.unlock();
        }
        // A started helper observes writer EOF and continues cleanup even if its
        // caller's future is dropped or Greenlight exits.
        self.control.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{atomic::AtomicBool, Arc},
        time::Duration,
    };

    #[test]
    fn resource_records_require_confirmed_cleanup_and_a_complete_known_schema() {
        let job = Job::fixture("/usr/bin/true");
        job.guard
            .lease
            .write_all_at(b"usage_v1 1 2 3 4 5\n", 256)
            .unwrap();
        assert!(job.guard.resources().is_none());
        job.guard.lease.write_all_at(b"complete", 0).unwrap();
        let usage = job.guard.resources().unwrap();
        assert_eq!(
            (
                usage.user_cpu_us,
                usage.system_cpu_us,
                usage.largest_child_maxrss_kib,
                usage.input_block_operations,
                usage.output_block_operations
            ),
            (1, 2, 3, 4, 5)
        );
        job.guard
            .lease
            .set_len(256 + b"usage_v1 1 2 3 4 5".len() as u64)
            .unwrap();
        assert!(job.guard.resources().is_none());
        job.guard.lease.set_len(256).unwrap();
        job.guard
            .lease
            .write_all_at(b"usage_v2 1 2 3 4 5\n", 256)
            .unwrap();
        assert!(job.guard.resources().is_none());
    }

    #[test]
    fn unfinished_jobs_block_recovery_even_after_their_lock_is_lost() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::anchored_fs::Dir::open(dir.path()).unwrap();
        let jobs = state.create_dirs(Path::new("sandbox-jobs")).unwrap();
        let file = jobs.lock_file(Path::new("interrupted.lease")).unwrap();
        file.write_all_at(b"pending ", 0).unwrap();
        assert!(ensure_clean(&state).unwrap_err().is::<CleanupIncomplete>());
        file.write_all_at(b"complete", 0).unwrap();
        file.try_lock().unwrap();
        assert!(ensure_clean(&state).unwrap_err().is::<CleanupIncomplete>());
        // Model a released lock explicitly. Other parallel tests may fork while
        // this descriptor is open; their pre-exec copy can outlive drop(file).
        file.unlock().unwrap();
        drop(file);
        ensure_clean(&state).unwrap();
        assert_eq!(jobs.entries().unwrap().count(), 0);
    }

    #[test]
    fn unused_commands_leave_no_false_interrupted_job() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::anchored_fs::Dir::open(dir.path()).unwrap();
        let jobs = state.create_dirs(Path::new("sandbox-jobs")).unwrap();
        drop(Job::new("/usr/bin/true", &jobs).unwrap());
        ensure_clean(&state).unwrap();
    }

    // This fixture creates the exact ownership problem without requiring working
    // user namespaces: the leader exits, leaving a session-escaped pipe holder.
    fn escaped_child(path: &Path, exit: bool) -> Job {
        let mut job = Job::fixture("/usr/bin/python3");
        job.args([
            "-c",
            r#"
import os, signal, sys, time
pid = os.fork()
if pid == 0:
    os.setsid()
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    with open(sys.argv[1], 'w') as f:
        f.write(str(os.getpid()))
    while True: time.sleep(1)
while not os.path.exists(sys.argv[1]): time.sleep(.001)
if sys.argv[2] == 'exit':
    sys.stderr.write('bwrap: fixture startup failure\n')
    sys.exit(1)
while True: time.sleep(1)
"#,
        ])
        .arg(path)
        .arg(if exit { "exit" } else { "wait" });
        job
    }
    async fn child_pid(path: &Path) -> i32 {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(path) {
                    if let Ok(pid) = text.parse() {
                        break pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }
    async fn assert_reaped(pid: i32) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if unsafe { libc::kill(pid, 0) } < 0 {
                    assert_eq!(
                        std::io::Error::last_os_error().raw_os_error(),
                        Some(libc::ESRCH)
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("session-escaped descendant was not reaped");
    }
    #[tokio::test]
    async fn supervisor_leaves_foreground_group_and_reaps_after_owner_drop() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("escaped-pid");
        let mut job = escaped_child(&path, false);
        job.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let (mut supervisor, owner) = job.spawn().unwrap();
        let descendant = child_pid(&path).await;
        let pid = supervisor.id().unwrap() as i32;
        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
        assert_ne!(unsafe { libc::getpgid(pid) }, unsafe { libc::getpgrp() });
        // Like application exit, this closes control without sending a signal
        // to the owner's isolated process group. No test-runner group is killed.
        drop(owner);
        tokio::time::timeout(Duration::from_secs(5), supervisor.wait())
            .await
            .unwrap()
            .unwrap();
        assert_reaped(descendant).await;
    }

    #[tokio::test]
    async fn startup_failure_reaps_session_escaped_pipe_holder() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("pid");
        let error = crate::sandbox::pipeline::run(
            vec![escaped_child(&pid_path, true)],
            vec!["fixture".into()],
            vec![],
            &Arc::new(AtomicBool::new(false)),
            Duration::from_secs(3),
        )
        .await
        .err()
        .unwrap();
        assert!(format!("{error:#}").contains("namespace setup failed"));
        assert_reaped(child_pid(&pid_path).await).await;
    }
    #[tokio::test]
    async fn dropping_execution_future_still_reaps_descendants_and_leaves_unrelated_child_alone() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("pid");
        let job = escaped_child(&pid_path, false);
        let mut unrelated = tokio::process::Command::new("/usr/bin/sleep")
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let task = tokio::spawn(async move {
            crate::sandbox::pipeline::run(
                vec![job],
                vec!["fixture".into()],
                vec![],
                &Arc::new(AtomicBool::new(false)),
                Duration::from_secs(30),
            )
            .await
        });
        let pid = child_pid(&pid_path).await;
        task.abort();
        let _ = task.await;
        assert_reaped(pid).await;
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().await.unwrap();
    }
    #[tokio::test]
    async fn cleanup_timeout_keeps_project_quarantined_until_supervisor_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::anchored_fs::Dir::open(dir.path()).unwrap();
        let jobs = state.create_dirs(Path::new("sandbox-jobs")).unwrap();
        let pid_path = dir.path().join("supervisor-pid");
        let mut job = Job::new("/usr/bin/python3", &jobs).unwrap();
        job.args([
            "-c",
            "import os,sys,time; open(sys.argv[1], 'w').write(str(os.getppid())); time.sleep(30)",
        ])
        .arg(&pid_path);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let task = tokio::spawn(async move {
            crate::sandbox::pipeline::run(
                vec![job],
                vec!["fixture".into()],
                vec![],
                &flag,
                Duration::from_secs(30),
            )
            .await
        });
        let pid = child_pid(&pid_path).await;
        assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let result = tokio::time::timeout(Duration::from_secs(8), task).await;
        let blocked = ensure_clean(&state).is_err();
        // Always release the fixture supervisor, even if the bounded wait fails.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGCONT) }, 0);
        assert!(blocked);
        let error = result.unwrap().unwrap().err().unwrap();
        assert!(error.is::<CleanupIncomplete>(), "{error:#}");
        tokio::time::timeout(Duration::from_secs(3), async {
            while ensure_clean(&state).is_err() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    #[test]
    fn parent_process_exit_triggers_cleanup_without_running_rust_destructors() {
        const FIXTURE: &str = "GREENLIGHT_GL033_PARENT_EXIT_FIXTURE";
        if let Some(root) = std::env::var_os(FIXTURE) {
            let state = crate::anchored_fs::Dir::open(Path::new(&root)).unwrap();
            let jobs = state.create_dirs(Path::new("sandbox-jobs")).unwrap();
            let path = Path::new(&root).join("child-pid");
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _enter = runtime.enter();
            let mut job = Job::new("/usr/bin/python3", &jobs).unwrap();
            job.args(["-c", "import os,sys,time; open(sys.argv[1], 'w').write(str(os.getpid())); time.sleep(30)"]).arg(&path);
            let (_child, _guard) = job.spawn().unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while std::fs::read_to_string(&path)
                .unwrap_or_default()
                .is_empty()
            {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            std::process::exit(0);
        }
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sandbox_job::tests::parent_process_exit_triggers_cleanup_without_running_rust_destructors"])
            .env(FIXTURE, dir.path()).output().unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        let pid: i32 = std::fs::read_to_string(dir.path().join("child-pid"))
            .unwrap()
            .parse()
            .unwrap();
        let state = crate::anchored_fs::Dir::open(dir.path()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if ensure_clean(&state).is_ok() && unsafe { libc::kill(pid, 0) } < 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "parent exit left an invocation running"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
