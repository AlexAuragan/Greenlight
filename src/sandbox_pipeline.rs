//! Structured pipe execution. Only Sandbox builds production commands; tests use
//! local fixed fixtures to exercise lifecycle behavior without namespace support.
use crate::sandbox_job::Job as Command;
use anyhow::{anyhow, Context, Result};
use std::os::unix::process::ExitStatusExt;
use std::{
    process::{ExitStatus, Output, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Child,
    sync::Notify,
    task::JoinSet,
};
const CAP: usize = 128 * 1024;

pub(crate) struct PipelineOutput {
    pub output: Output,
    pub summary: String,
}

async fn capture<R: AsyncRead + Unpin>(
    mut reader: R,
    budget: Arc<AtomicUsize>,
    overflow: Arc<AtomicBool>,
    wake: Arc<Notify>,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let mut current = budget.load(Ordering::SeqCst);
        let left = loop {
            match budget.compare_exchange_weak(
                current,
                current.saturating_sub(count),
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(previous) => break previous,
                Err(actual) => current = actual,
            }
        };
        let keep = count.min(left);
        bytes.extend_from_slice(&buffer[..keep]);
        if keep < count {
            overflow.store(true, Ordering::SeqCst);
            wake.notify_one();
            break;
        }
    }
    Ok(bytes)
}

// An OS-pipe read must stay nonblocking. tokio::fs uses blocking worker reads,
// which cannot be cancelled when a failed startup helper keeps a writer open.
struct Pipe(tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>);
impl Pipe {
    fn new(fd: std::os::fd::OwnedFd) -> std::io::Result<Self> {
        use std::os::fd::AsRawFd;
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: we own this nonblocking FD; it is not closed or replaced
        // before the registration is dropped. The downstream reader has exited.
        Ok(Self(unsafe { tokio::io::unix::AsyncFd::register(fd) }?))
    }
}
impl AsyncRead for Pipe {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::os::fd::AsRawFd;
        loop {
            let mut ready = std::task::ready!(self.0.poll_read_ready(cx))?;
            match ready.try_io(|fd| {
                let bytes = buf.initialize_unfilled();
                let count = unsafe {
                    libc::read(
                        fd.get_ref().as_raw_fd(),
                        bytes.as_mut_ptr().cast(),
                        bytes.len(),
                    )
                };
                if count < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(count as usize)
                }
            }) {
                Ok(Ok(count)) => {
                    buf.advance(count);
                    return std::task::Poll::Ready(Ok(()));
                }
                Ok(Err(error)) => return std::task::Poll::Ready(Err(error)),
                Err(_) => continue,
            }
        }
    }
}

enum Captured {
    Stdout(Vec<u8>),
    Stderr(usize, Vec<u8>),
    Discard(usize, u64),
}
type ReaderResult = std::io::Result<Captured>;
fn retain(
    result: std::result::Result<ReaderResult, tokio::task::JoinError>,
    stdout: &mut Vec<u8>,
    stderr: &mut [Vec<u8>],
    discarded: &mut [u64],
) -> Result<()> {
    match result.context("Pipeline capture task failed")?? {
        Captured::Stdout(bytes) => *stdout = bytes,
        Captured::Stderr(index, bytes) => stderr[index] = bytes,
        Captured::Discard(edge, bytes) => discarded[edge] = bytes,
    }
    Ok(())
}

pub(crate) async fn run(
    commands: Vec<Command>,
    labels: Vec<String>,
    heads: Vec<usize>,
    stop: &Arc<AtomicBool>,
    deadline: Duration,
) -> Result<PipelineOutput> {
    if commands.is_empty()
        || commands.len() > 4
        || commands.len() != labels.len()
        || heads.iter().any(|h| *h >= commands.len())
    {
        return Err(anyhow!("Invalid validated pipeline shape"));
    }
    let count = commands.len();
    let mut children: Vec<Child> = Vec::new();
    let mut owners = Vec::new();
    let mut readers: JoinSet<ReaderResult> = JoinSet::new();
    let mut guards = Vec::new();
    let mut discarded = vec![0u64; count - 1];
    let mut stdout = Vec::new();
    let mut stderr = vec![Vec::new(); count];
    let stderr_budget = Arc::new(AtomicUsize::new(CAP));
    let overflow = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(Notify::new());
    let started = tokio::time::Instant::now();
    // The closure owns every unused pipe/Command end, so even a partial spawn
    // failure closes parent copies before cleanup waits for the started children.
    let launch = (|| -> Result<()> {
        let mut previous = None;
        for (index, mut command) in commands.into_iter().enumerate() {
            if stop.load(Ordering::SeqCst) {
                return Err(anyhow!(
                    "Sandbox execution stopped by user before all stages started"
                ));
            }
            if started.elapsed() >= deadline {
                return Err(anyhow!("Sandbox launch/execution deadline reached"));
            }
            command.stderr(Stdio::piped());
            command.stdin(previous.take().map_or_else(Stdio::null, Stdio::from));
            if index + 1 < count {
                let (reader, writer) = std::io::pipe()?;
                // Keep the producer alive after an early consumer exit. Drain
                // only after that consumer exits, never competing for its input.
                guards.push(Some(reader.try_clone()?));
                command.stdout(Stdio::from(writer));
                previous = Some(reader);
            } else {
                command.stdout(Stdio::piped());
            }
            let (mut child, owner) = command.spawn().with_context(|| {
                format!(
                    "Pipeline stage {} ({}) could not start",
                    index + 1,
                    labels[index]
                )
            })?;
            owners.push(owner);
            let error = child.stderr.take().context("Missing pipeline stderr")?;
            let budget = stderr_budget.clone();
            let flag = overflow.clone();
            let notify = wake.clone();
            readers.spawn(async move {
                Ok(Captured::Stderr(
                    index,
                    capture(error, budget, flag, notify).await?,
                ))
            });
            if index + 1 == count {
                let output = child.stdout.take().context("Missing pipeline stdout")?;
                let flag = overflow.clone();
                let notify = wake.clone();
                readers.spawn(async move {
                    Ok(Captured::Stdout(
                        capture(output, Arc::new(AtomicUsize::new(CAP)), flag, notify).await?,
                    ))
                });
            }
            children.push(child);
        }
        Ok(())
    })();
    let mut outcome = if let Err(error) = launch {
        Err(error)
    } else {
        let complete = async {
            loop {
                let mut finished = true;
                for (index, child) in children.iter_mut().enumerate() {
                    let status = child.try_wait()?;
                    finished &= status.is_some();
                    if status.is_some() && index > 0 {
                        if let Some(guard) = guards[index - 1].take() {
                            let fd: std::os::fd::OwnedFd = guard.into();
                            let mut input = Pipe::new(fd)?;
                            readers.spawn(async move {
                                let bytes =
                                    tokio::io::copy(&mut input, &mut tokio::io::sink()).await?;
                                Ok(Captured::Discard(index - 1, bytes))
                            });
                        }
                    }

                    if status.is_some_and(|s| !s.success())
                        && String::from_utf8_lossy(&stderr[index])
                            .trim_start()
                            .starts_with("bwrap:")
                    {
                        return Err(anyhow!("Bubblewrap namespace setup failed; stages may have started. No host fallback"));
                    }
                }
                if finished {
                    while let Some(result) = readers.join_next().await {
                        retain(result, &mut stdout, &mut stderr, &mut discarded)?;
                    }
                    break;
                }
                // At most four children. Polling avoids borrowing Child handles
                // in detached wait tasks, so cancellation can always kill/reap them.
                tokio::select! {
                    Some(result)=readers.join_next(), if !readers.is_empty()=>retain(result,&mut stdout,&mut stderr,&mut discarded)?,
                    _=tokio::time::sleep(Duration::from_millis(10))=>{},
                }
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::select! {
            biased;
            _=crate::provider_failure::cancelled(stop)=>Err(anyhow!("Sandbox execution stopped by user; no automatic replay")),
            _=tokio::time::sleep_until(started+deadline)=>Err(anyhow!("Sandbox launch/execution deadline reached")),
            _=wake.notified()=>Err(anyhow!("Sandbox output limit reached; output is incomplete")),
            result=complete=>result,
        }
    };
    if overflow.load(Ordering::SeqCst) && outcome.is_ok() {
        outcome = Err(anyhow!(
            "Sandbox output limit reached; output is incomplete"
        ));
    }
    if outcome.is_err() {
        for owner in &mut owners {
            owner.cancel();
        }
    }
    let mut statuses: Vec<ExitStatus> = Vec::new();
    let cleanup = async {
        for child in &mut children {
            statuses.push(child.wait().await?);
        }
        for owner in &owners {
            owner.confirmed()?;
        }
        while let Some(result) = readers.join_next().await {
            retain(result, &mut stdout, &mut stderr, &mut discarded)?;
        }
        Ok::<_, anyhow::Error>(())
    };
    match tokio::time::timeout(Duration::from_secs(5), cleanup).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return Err(anyhow::Error::new(crate::sandbox_job::CleanupIncomplete)
                .context(format!("Sandbox cleanup failed: {error:#}")))
        }
        Err(_) => {
            for owner in &mut owners {
                owner.cancel();
            }
            // Dropping nonblocking readers cannot strand a blocking-pool read.
            // The detached supervisor retains the job lease until it has reaped
            // every descendant; recovery remains blocked in the meantime.
            readers.abort_all();
            return Err(crate::sandbox_job::CleanupIncomplete.into());
        }
    }
    let namespace_failed = statuses.iter().enumerate().any(|(i, status)| {
        !status.success()
            && String::from_utf8_lossy(&stderr[i])
                .trim_start()
                .starts_with("bwrap:")
    });
    if namespace_failed && outcome.is_ok() {
        outcome = Err(anyhow!(
            "Bubblewrap namespace setup failed; stages may have started. No host fallback"
        ));
    }
    let real_failure = statuses.iter().rposition(|s| !s.success());
    if outcome.is_ok()
        && real_failure.is_none()
        && discarded
            .iter()
            .enumerate()
            .any(|(edge, bytes)| *bytes > 0 && !heads.contains(&(edge + 1)))
    {
        outcome = Err(anyhow!(
            "Pipeline input closed early outside a supported head short read; output is incomplete"
        ));
    }
    let mut summary = String::from(if count == 1 {
        "[Sandbox command]\n"
    } else {
        "[Validated pipeline stages]\n"
    });
    for (i, status) in statuses.iter().enumerate() {
        summary.push_str(&format!(
            "{} · {} · {}\n",
            i + 1,
            labels[i],
            status
                .code()
                .map(|c| format!("exit_code: {c}"))
                .unwrap_or_else(|| format!("signal: {}", status.signal().unwrap_or(-1)))
        ));
    }
    for (edge, bytes) in discarded
        .iter()
        .enumerate()
        .filter(|(_, bytes)| **bytes > 0)
    {
        summary.push_str(&format!(
            "Stage {} closed input; drained {} remaining upstream byte(s){}\n",
            edge + 2,
            bytes,
            if outcome.is_ok() && real_failure.is_none() && heads.contains(&(edge + 1)) {
                " after successful head (expected short read)"
            } else {
                "; preserved the stage outcomes"
            }
        ));
    }
    if count == 1 {
        summary.push_str(&format!("Started {} of 1 command.\n", children.len()));
    } else {
        summary.push_str(&format!(
            "Started {} of {count} stages; intermediate data stayed in OS pipes.\n",
            children.len()
        ));
    }
    let mut errors = Vec::new();
    for (i, bytes) in stderr.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
        errors.extend_from_slice(format!("[Stage {} · {} stderr]\n", i + 1, labels[i]).as_bytes());
        errors.extend_from_slice(bytes);
        errors.push(b'\n');
    }
    if let Err(error) = outcome {
        let kind = if count == 1 {
            "Sandbox command"
        } else {
            "Pipeline"
        };
        let details=format!(
            "{kind} did not complete: {error}\n{summary}\nPartial stdout:\n{}\nCaptured stderr:\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&errors)
        );
        return Err(error.context(details));
    }
    let status = statuses[real_failure.unwrap_or(count - 1)];
    Ok(PipelineOutput {
        output: Output {
            status,
            stdout,
            stderr: errors,
        },
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command(program: &str, args: &[&str]) -> Command {
        let mut command = Command::fixture(program);
        command.args(args).env_clear();
        command
    }
    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }
    #[tokio::test]
    async fn pipes_preserve_data_and_every_stage_status_without_a_shell() {
        let output = run(
            vec![
                command("/usr/bin/printf", &["b\na\na\n"]),
                command("/usr/bin/sort", &[]),
                command("/usr/bin/uniq", &["-c"]),
            ],
            labels(&["printf", "sort", "uniq"]),
            vec![],
            &Arc::new(AtomicBool::new(false)),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert!(output.output.status.success());
        let text = String::from_utf8(output.output.stdout).unwrap();
        assert!(text.contains("2 a"));
        assert!(text.contains("1 b"));
        assert_eq!(output.summary.matches("exit_code: 0").count(), 3);
    }
    #[tokio::test]
    async fn a_later_success_never_hides_upstream_failure_or_no_match() {
        let stop = Arc::new(AtomicBool::new(false));
        let output = run(
            vec![
                command("/usr/bin/false", &[]),
                command("/usr/bin/wc", &["-l"]),
            ],
            labels(&["false", "wc"]),
            vec![],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert_eq!(output.output.status.code(), Some(1));
        assert!(output.summary.contains("2 · wc · exit_code: 0"));
        let output = run(
            vec![
                command("/usr/bin/printf", &["no match\n"]),
                command("/usr/bin/grep", &["absent"]),
                command("/usr/bin/head", &["-n", "1"]),
            ],
            labels(&["printf", "grep", "head"]),
            vec![2],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert_eq!(output.output.status.code(), Some(1));
        assert!(output.summary.contains("2 · grep · exit_code: 1"));
    }

    #[tokio::test]
    async fn successful_head_drains_without_guessing_from_exit_141() {
        let stop = Arc::new(AtomicBool::new(false));
        let output = run(
            vec![
                command("/usr/bin/seq", &["1", "100000"]),
                command("/usr/bin/head", &["-n", "1"]),
            ],
            labels(&["seq", "head"]),
            vec![1],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert!(output.output.status.success());
        assert_eq!(output.output.stdout, b"1\n");
        assert!(output.summary.contains("expected short read"));
        let error = run(
            vec![
                command("/usr/bin/seq", &["1", "100000"]),
                command("/usr/bin/head", &["-n", "1"]),
            ],
            labels(&["seq", "reader"]),
            vec![],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .err()
        .unwrap();
        assert!(format!("{error:#}").contains("outside a supported head"));
        let output = run(
            vec![
                command("/bin/sh", &["-c", "exit 141"]),
                command("/usr/bin/head", &["-n", "1"]),
            ],
            labels(&["fixture", "head"]),
            vec![1],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert_eq!(output.output.status.code(), Some(141));
        assert!(!output.summary.contains("expected short read"));
    }
    #[tokio::test]
    async fn partial_launch_failure_kills_and_reaps_started_processes() {
        let error = run(
            vec![command("/usr/bin/sleep", &["30"]), {
                let mut job = command("/usr/bin/true", &[]);
                job.command =
                    tokio::process::Command::new("/definitely-missing-greenlight-fixture");
                job
            }],
            labels(&["sleep", "missing"]),
            vec![],
            &Arc::new(AtomicBool::new(false)),
            Duration::from_secs(3),
        )
        .await
        .err()
        .unwrap();
        let text = format!("{error:#}");
        assert!(text.contains("could not start"));
        assert!(text.contains("Started 1 of 2"));
        assert!(text.contains("signal: 9"));
    }
    #[tokio::test]
    async fn cancellation_and_shared_deadline_reap_the_entire_pipeline() {
        for cancel in [false, true] {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let setter = tokio::spawn(async move {
                if cancel {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    flag.store(true, Ordering::SeqCst);
                }
            });
            let error = run(
                vec![
                    command("/usr/bin/sleep", &["30"]),
                    command("/usr/bin/wc", &["-l"]),
                ],
                labels(&["sleep", "wc"]),
                vec![],
                &stop,
                if cancel {
                    Duration::from_secs(3)
                } else {
                    Duration::from_millis(30)
                },
            )
            .await
            .err()
            .unwrap();
            setter.await.unwrap();
            let text = format!("{error:#}");
            assert!(text.contains(if cancel {
                "stopped by user"
            } else {
                "deadline"
            }));
            assert!(text.contains("Started 2 of 2"));
            assert!(text.contains("signal: 9"));
        }
    }
    #[tokio::test]
    async fn final_stdout_and_combined_stderr_have_shared_hard_bounds() {
        let stop = Arc::new(AtomicBool::new(false));
        let error = run(
            vec![
                command("/usr/bin/head", &["-c", "1000000", "/dev/zero"]),
                command("/usr/bin/cat", &[]),
            ],
            labels(&["bytes", "cat"]),
            vec![],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .err()
        .unwrap();
        assert!(format!("{error:#}").contains("output limit"));
        // Static test fixtures only: production never accepts an interpreter.
        let error = run(
            vec![
                command(
                    "/bin/sh",
                    &["-c", "head -c 80000 /dev/zero >&2; printf data"],
                ),
                command("/bin/sh", &["-c", "head -c 80000 /dev/zero >&2; cat"]),
            ],
            labels(&["errors1", "errors2"]),
            vec![],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .err()
        .unwrap();
        let text = format!("{error:#}");
        assert!(text.contains("output limit"));
        assert!(text.len() < CAP * 2 + 4096);
    }
    #[tokio::test]
    async fn stderr_pressure_is_drained_concurrently_and_namespace_errors_fail_closed() {
        let stop = Arc::new(AtomicBool::new(false));
        let output = run(
            vec![
                command(
                    "/bin/sh",
                    &["-c", "head -c 70000 /dev/zero >&2; printf data"],
                ),
                command("/usr/bin/wc", &["-c"]),
            ],
            labels(&["fixture", "wc"]),
            vec![],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert!(output.output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.output.stdout).trim(), "4");
        let error = run(
            vec![
                command(
                    "/bin/sh",
                    &[
                        "-c",
                        "printf 'bwrap: fixture isolation failure' >&2; exit 1",
                    ],
                ),
                command("/usr/bin/wc", &["-l"]),
            ],
            labels(&["bwrap", "wc"]),
            vec![],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .err()
        .unwrap();
        assert!(format!("{error:#}").contains("namespace setup failed"));
        let error = run(
            vec![
                command("/usr/bin/sleep", &["30"]),
                command(
                    "/bin/sh",
                    &["-c", "printf 'bwrap: later namespace failed' >&2; exit 1"],
                ),
            ],
            labels(&["sleep", "bwrap"]),
            vec![],
            &stop,
            Duration::from_secs(3),
        )
        .await
        .err()
        .unwrap();
        let text = format!("{error:#}");
        assert!(text.contains("namespace setup failed"));
        assert!(text.contains("1 · sleep · signal: 9"));
    }
    #[tokio::test]
    async fn parsed_pipeline_uses_one_unchanged_view_and_correct_stdin_argv() {
        use std::{fs, os::unix::fs::symlink};
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(root.path().join("notes"), "b\na\na\n").unwrap();
        fs::write(outside.path().join("secret"), "outside").unwrap();
        let plan =
            crate::project_inspection::parse(root.path(), "cat notes | sort | uniq -c | head -n 2")
                .unwrap();
        let view = plan.snapshot(root.path()).unwrap();
        fs::remove_file(root.path().join("notes")).unwrap();
        symlink(outside.path().join("secret"), root.path().join("notes")).unwrap();
        let commands = plan
            .stages()
            .iter()
            .map(|argv| {
                let mut cmd = Command::fixture(&argv[0]);
                cmd.args(&argv[1..]).current_dir(view.path()).env_clear();
                cmd
            })
            .collect();
        let output = run(
            commands,
            labels(&["cat", "sort", "uniq", "head"]),
            plan.head_stages(),
            &Arc::new(AtomicBool::new(false)),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert!(output.output.status.success());
        let text = String::from_utf8(output.output.stdout).unwrap();
        assert!(text.contains("2 a"));
        assert!(text.contains("1 b"));
        assert!(!text.contains("outside"));
        if std::path::Path::new("/usr/bin/rg").is_file() {
            fs::remove_file(root.path().join("notes")).unwrap();
            fs::write(root.path().join("notes"), "a\nb\n").unwrap();
            let plan = crate::project_inspection::parse(root.path(), "cat notes | rg a").unwrap();
            let view = plan.snapshot(root.path()).unwrap();
            let commands = plan
                .stages()
                .iter()
                .map(|argv| {
                    let mut cmd = Command::fixture(&argv[0]);
                    cmd.args(&argv[1..]).current_dir(view.path()).env_clear();
                    cmd
                })
                .collect();
            let output = run(
                commands,
                labels(&["cat", "rg"]),
                vec![],
                &Arc::new(AtomicBool::new(false)),
                Duration::from_secs(3),
            )
            .await
            .unwrap();
            assert_eq!(output.output.stdout, b"a\n");
        }
    }
}
