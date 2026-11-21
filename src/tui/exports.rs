//! Explicit user exports. Original artifacts/history are never edited or replayed.
use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

pub(super) struct Snapshot {
    pub label: String,
    pub text: String,
}
impl App {
    pub(super) fn export_snapshot(&self) -> Result<Snapshot> {
        let (label, text) = if self.help {
            ("keyboard help".into(), help_text().into())
        } else if self.shell.sessions {
            (
                "session browser snapshot · candidates only".into(),
                format!(
                    "Session candidates · query {}\n{}",
                    self.session_query,
                    self.navigation
                        .candidates
                        .iter()
                        .map(|s| format!("{} · {} · {}", s.goal, s.status, s.current_step))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            )
        } else if self.human_review().is_some() {
            (
                "human review · selected recorded evidence".into(),
                self.review_document_text().unwrap_or_default(),
            )
        } else if let Some(text) = &self.diagnostic {
            ("diagnostic".into(), text.clone())
        } else if let Some(page) = self.details.page {
            (
                format!("{page:?} · selected session/branch snapshot"),
                if page == details::Page::Conversation {
                    self.details
                        .dump
                        .as_ref()
                        .map(|d| d.markdown())
                        .transpose()?
                        .unwrap_or_else(|| self.details.text.clone())
                } else {
                    self.details.text.clone()
                },
            )
        } else if let Some(text) = &self.context_detail {
            ("attempt context".into(), text.clone())
        } else if let Some(browser) = &self.artifact_browser {
            (
                format!(
                    "artifact {} event #{} {:?}",
                    browser.name, browser.versions[browser.index].event_id, browser.mode
                ),
                browser.error.as_ref().unwrap_or(&browser.text).clone(),
            )
        } else if let Some(pending) = &self.edits.pending {
            ("revert preview".into(), pending.preview.clone())
        } else if self.result_open && self.view == View::Conversation {
            ("workflow result".into(), self.result_text.clone())
        } else if self.view == View::Edits && self.edits.overview && self.edits.error.is_none() {
            ("recorded native edits overview".into(),format!("Edits · selected session / active branch\nNative journal only; shell/manual changes are outside coverage.\n{}\n\nKeep records historical review intent, not live disk validation, tool permission or Git staging.",self.edits.files.iter().map(|f|format!("{} · {} · revision #{} · {}",f.path,f.label(),f.revision,if f.kept{"Kept revision"}else{"Not kept"})).collect::<Vec<_>>().join("\n")))
        } else if self.view == View::Edits {
            (
                "recorded code diff".into(),
                self.edits
                    .error
                    .as_ref()
                    .unwrap_or(&self.edits.text)
                    .clone(),
            )
        } else if self.restart_plan.is_some() {
            ("restore preview".into(), self.restart_preview.clone())
        } else if !self.restart_choices.is_empty() {
            bail!("Choose a restore preview before exporting")
        } else if self.view == View::Artifacts {
            if let Some(error) = &self.artifact_error {
                bail!("{error}");
            }
            let artifact = self.artifact().context("No artifact selected")?;
            (
                format!("artifact {} event #{}", artifact.name, artifact.event_id),
                artifact.content.clone(),
            )
        } else if self.blocker_card && self.workspace.as_ref().is_some_and(|w| w.blocker.is_some())
        {
            (
                "blocked session context".into(),
                self.workspace.as_ref().unwrap().context(),
            )
        } else {
            let event = self.event().context("No activity selected")?;
            if self.partial_history && self.exact_event.as_ref().is_none_or(|e| e.id != event.id) {
                bail!("Open the exact event body before exporting; the current row is a labelled preview");
            }
            (format!("activity event #{}", event.id), self.detail())
        };
        Ok(Snapshot { label, text })
    }
    pub(super) fn start_copy(&mut self) {
        if self.copy_job.is_some() {
            self.notice = "A clipboard request is already running.".into();
            return;
        }
        match self.export_snapshot() {
            Ok(snapshot) => {
                let (tx, rx) = mpsc::channel();
                self.copy_job = Some(rx);
                thread::spawn(move || {
                    let result = copy_text(&snapshot.text)
                        .map(|backend| {
                            format!(
                                "Copied {} via {backend} (this machine's clipboard).",
                                inline_display(&snapshot.label)
                            )
                        })
                        .map_err(|error| format!("Copy failed: {error:#}"));
                    let _ = tx.send(result);
                });
                self.notice = "Copying full selected text…".into();
            }
            Err(error) => self.notice = format!("Nothing copied: {error:#}"),
        }
    }
    pub(super) fn poll_copy(&mut self) {
        let Some(job) = &self.copy_job else {
            return;
        };
        match job.try_recv() {
            Ok(result) => {
                self.copy_job = None;
                match result {
                    Ok(message) => self.notice = message,
                    Err(error) => self.raise_diagnostic(error),
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.copy_job = None;
                self.raise_diagnostic("Clipboard worker stopped; copy was not confirmed.".into());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    pub(super) fn request_editor(&mut self) {
        if self.runner.is_some() {
            self.notice="Wait for this TUI's worker before opening the editor, so approvals stay available.".into();
            return;
        }
        match self.export_snapshot() {
            Ok(snapshot) => self.pending_editor = Some(snapshot),
            Err(error) => self.notice = format!("Nothing opened: {error:#}"),
        }
    }
}
fn copy_command(program: &str, args: &[&str], text: &str) -> Result<()> {
    copy_command_with_timeout(program, args, text, Duration::from_secs(2))
}
fn copy_command_with_timeout(
    program: &str,
    args: &[&str],
    text: &str,
    timeout: Duration,
) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut input = child.stdin.take().context("clipboard stdin unavailable")?;
    let text = text.to_owned();
    let writer = thread::spawn(move || input.write_all(text.as_bytes()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("clipboard utility timed out")
        }
        thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        bail!("clipboard utility rejected the copy; check the desktop session")
    }
    while !writer.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if writer.is_finished() {
        writer
            .join()
            .map_err(|_| anyhow::anyhow!("clipboard writer stopped"))??;
    } else {
        bail!("clipboard utility did not consume the text")
    }
    Ok(())
}
fn copy_text(text: &str) -> Result<&'static str> {
    let mut failures = Vec::new();
    for (program, args) in [
        ("wl-copy", vec!["--type", "text/plain;charset=utf-8"]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        match copy_command(program, &args, text) {
            Ok(()) => return Ok(program),
            Err(error) => failures.push(format!("{program}: {error:#}")),
        }
    }
    bail!("No usable clipboard utility (wl-copy/xclip/xsel). Check the desktop session or export the selected record with the CLI.\n\n{}",failures.join("\n"))
}
fn editor_command(setting: &str, path: &Path) -> Result<Command> {
    let words = shlex::split(setting).context("Editor setting has unmatched quotes")?;
    let program = words
        .first()
        .filter(|s| !s.is_empty())
        .context("Set VISUAL or EDITOR to an editor command")?;
    let mut command = Command::new(program);
    command.args(&words[1..]);
    let name = Path::new(program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if matches!(
        name,
        "code" | "code-insiders" | "codium" | "cursor" | "subl"
    ) && !words
        .iter()
        .any(|arg| matches!(arg.as_str(), "--wait" | "-w"))
    {
        command.arg("--wait");
    }
    if matches!(name, "gvim" | "mvim")
        && !words
            .iter()
            .any(|arg| matches!(arg.as_str(), "-f" | "--nofork"))
    {
        command.arg("-f");
    }
    command
        .arg(path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(parent) = path.parent() {
        command.current_dir(parent);
    }
    Ok(command)
}
pub(super) fn open_editor(snapshot: &Snapshot) -> Result<()> {
    let setting = std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".into());
    open_editor_with(snapshot, &setting)
}
fn snapshot_file(snapshot: &Snapshot) -> Result<tempfile::NamedTempFile> {
    let mut file = tempfile::Builder::new()
        .prefix("greenlight-review-")
        .suffix(".txt")
        .tempfile()?;
    file.write_all(snapshot.text.as_bytes())?;
    file.flush()?;
    let mut permissions = file.as_file().metadata()?.permissions();
    permissions.set_readonly(true);
    file.as_file().set_permissions(permissions)?;
    Ok(file)
}
fn open_editor_with(snapshot: &Snapshot, setting: &str) -> Result<()> {
    let file = snapshot_file(snapshot)?;
    let status = editor_command(setting, file.path())?
        .status()
        .context("Could not start the configured editor")?;
    if !status.success() {
        bail!("Editor exited unsuccessfully; original content was not changed")
    }
    Ok(())
}
struct TerminalResume {
    active: bool,
}
impl TerminalResume {
    fn restore(&mut self) -> Result<()> {
        let screen = crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::cursor::Hide,
            crossterm::cursor::SetCursorStyle::BlinkingBar
        );
        let raw = crossterm::terminal::enable_raw_mode();
        self.active = false;
        screen?;
        raw?;
        Ok(())
    }
}
impl Drop for TerminalResume {
    fn drop(&mut self) {
        if self.active {
            let _ = self.restore();
        }
    }
}
pub(super) fn with_terminal_suspended(action: impl FnOnce() -> Result<()>) -> Result<Result<()>> {
    crossterm::terminal::disable_raw_mode()?;
    let mut resume = TerminalResume { active: true };
    crossterm::execute!(
        io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;
    let result = action();
    resume.restore().context("Could not restore TUI terminal")?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn script(root: &Path, name: &str, body: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    #[test]
    fn clipboard_sends_verbatim_stdin_without_interpreting_text_or_paths() {
        let root = tempfile::tempdir().unwrap();
        let program = script(root.path(), "fake copy", "cat > \"$1\"");
        let output = root.path().join("copied ; literal.txt");
        let text = "$(touch should-not-exist)\n漢字\u{001b}[2J";
        copy_command(program.to_str().unwrap(), &[output.to_str().unwrap()], text).unwrap();
        assert_eq!(std::fs::read_to_string(output).unwrap(), text);
        assert!(!root.path().join("should-not-exist").exists());
    }
    #[test]
    fn clipboard_failures_and_timeouts_are_not_reported_as_success() {
        let root = tempfile::tempdir().unwrap();
        let program = script(root.path(), "blocked copy", "exec sleep 5");
        assert!(copy_command_with_timeout(
            program.to_str().unwrap(),
            &[],
            "text",
            Duration::from_millis(50)
        )
        .unwrap_err()
        .to_string()
        .contains("timed out"));
        let program = script(root.path(), "failed copy", "exit 1");
        assert!(copy_command(program.to_str().unwrap(), &[], "text").is_err());
    }
    #[test]
    fn editor_argv_is_quoted_without_shell_expansion_and_gui_wait_is_present() {
        let command = editor_command(
            "'/path/editor with spaces' --flag 'two words'",
            Path::new("/tmp/name ; literal.txt"),
        )
        .unwrap();
        assert_eq!(command.get_program(), "/path/editor with spaces");
        assert_eq!(
            command
                .get_args()
                .map(|a| a.to_str().unwrap())
                .collect::<Vec<_>>(),
            ["--flag", "two words", "/tmp/name ; literal.txt"]
        );
        let command = editor_command("code", Path::new("/tmp/file.txt")).unwrap();
        assert!(command.get_args().any(|a| a == "--wait"));
        assert!(editor_command("'unterminated", Path::new("/tmp/file")).is_err());
    }
    #[test]
    fn editor_receives_a_private_temporary_snapshot_and_no_changes_are_imported() {
        let root = tempfile::tempdir().unwrap();
        let program=script(root.path(),"fake editor","printf '%s' \"$2\" > \"$1.path\"\ncat \"$2\" > \"$1.body\"\nrm \"$2\"\nprintf changed > \"$2\"");
        let output = root.path().join("capture");
        let snapshot = Snapshot {
            label: "artifact report".into(),
            text: "original\n漢字\u{001b}[2J".into(),
        };
        let file = snapshot_file(&snapshot).unwrap();
        assert_eq!(
            file.as_file().metadata().unwrap().permissions().mode() & 0o777,
            0o400
        );
        drop(file);
        open_editor_with(
            &snapshot,
            &format!("'{}' '{}'", program.display(), output.display()),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(output.with_extension("body")).unwrap(),
            snapshot.text
        );
        let path = std::fs::read_to_string(output.with_extension("path")).unwrap();
        assert!(!Path::new(&path).exists());
        assert!(snapshot.text.starts_with("original"));
    }
}
