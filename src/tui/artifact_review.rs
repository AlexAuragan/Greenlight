//! External editor drafts are private and disposable until publication succeeds.
use super::*;
use std::io::Write;

pub(super) struct Job {
    session: Session,
    name: String,
    source: i64,
    latest: i64,
    comment: bool,
    original: String,
    directory: tempfile::TempDir,
}
impl Job {
    pub(super) fn edit(&self) -> Result<String> {
        let setting = std::env::var("VISUAL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                std::env::var("EDITOR")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or_else(|| "vi".into());
        self.edit_with(&setting)
    }
    fn edit_with(&self, setting: &str) -> Result<String> {
        let status =
            exports::editor_command(setting, &self.directory.path().join("draft.md"))?.status()?;
        if !status.success() {
            bail!("Editor exited unsuccessfully; nothing imported");
        }
        let directory = crate::anchored_fs::Dir::open(self.directory.path())?;
        directory.read_string(
            Path::new("draft.md"),
            if self.comment {
                crate::artifact_review::COMMENT_BYTES as u64
            } else {
                crate::artifact_review::EDIT_BYTES as u64
            },
        )
    }
}
impl App {
    pub(super) fn prepare_artifact_review(&mut self, store: &Store, comment: bool) -> Result<()> {
        if self.runner.is_some() || self.manager_job.is_some() || self.summary_running() {
            bail!("Stop active workers before opening an artifact draft");
        }
        store.ensure_project_available()?;
        let session = self.session().cloned().context("No session selected")?;
        let (name, source) = if let Some(browser) = &self.artifact_browser {
            if browser.session != session.id || browser.branch != session.active_branch {
                bail!("Artifact branch changed");
            }
            (
                browser.name.clone(),
                browser
                    .versions
                    .get(browser.index)
                    .context("No selected version")?
                    .event_id,
            )
        } else {
            let selected = self.artifact().context("No selected artifact")?;
            (selected.name.clone(), selected.event_id)
        };
        let latest = store
            .artifact_versions(&session, &name)?
            .first()
            .context("No latest version")?
            .event_id;
        let artifact = store.artifact_version(&session, &name, source)?;
        let original = if comment {
            String::new()
        } else {
            artifact.content
        };
        if original.len() > crate::artifact_review::EDIT_BYTES {
            bail!("Artifact exceeds the 1 MiB edit limit");
        }
        let directory = tempfile::Builder::new()
            .prefix("greenlight-artifact-review-")
            .tempdir()?;
        let mut draft =
            crate::anchored_fs::Dir::open(directory.path())?.create_new(Path::new("draft.md"))?;
        draft.write_all(original.as_bytes())?;
        draft.sync_all()?;
        self.pending_artifact_review = Some(Job {
            session,
            name,
            source,
            latest,
            comment,
            original,
            directory,
        });
        Ok(())
    }
    pub(super) fn finish_artifact_review(
        &mut self,
        store: &Store,
        job: Job,
        edited: Result<String>,
    ) {
        let result = edited.and_then(|text| {
            if text == job.original || job.comment && text.trim().is_empty() {
                return Ok(false);
            }
            // The store also checks branch identity under the project control lease.
            if job.comment {
                store.comment_artifact(&job.session, &job.name, job.source, &text)
            } else {
                store.edit_artifact(&job.session, &job.name, job.source, job.latest, &text)
            }
        });
        match result {
            Ok(saved) => {
                let comment = job.comment;
                let name = job.name.clone();
                let session = job.session.clone();
                let job_source = job.source;
                drop(job);
                if saved {
                    if let Err(error) = self.refresh(store) {
                        self.notice = format!("Review saved; refresh failed: {error:#}");
                        return;
                    }
                    let mode = if comment {
                        crate::artifact_history::Mode::Comments
                    } else {
                        crate::artifact_history::Mode::Content
                    };
                    match crate::artifact_history::Browser::open(store, &session, &name) {
                        Ok(mut browser) => {
                            if comment {
                                browser.index = browser
                                    .versions
                                    .iter()
                                    .position(|v| v.event_id == job_source)
                                    .unwrap_or(0);
                            }
                            browser.mode = mode;
                            browser.reading = true;
                            browser.load(store, &session);
                            self.artifact_browser = Some(browser);
                            self.detail_scroll = 0;
                        }
                        Err(error) => {
                            self.notice = format!("Review saved; reader unavailable: {error:#}");
                            return;
                        }
                    }
                }
                self.notice = if !saved {"Draft unchanged or empty; nothing saved."}
                    else if comment {"User review comment saved on the exact version. It was not sent to the agent."}
                    else {"New user-authored artifact version saved. Original preserved; verification unchanged."}.into();
            }
            Err(error) => {
                let path = job.directory.keep().join("draft.md");
                self.raise_diagnostic(format!(
                    "Artifact review not saved: {error:#}\nPrivate draft retained for recovery: {}",
                    path.display()
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn setup() -> (tempfile::TempDir, Store, App, i64) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "plan").unwrap();
        store
            .add_artifact(&s, "plan", "plan.md", "original")
            .unwrap();
        let base = store.artifact_versions(&s, "plan.md").unwrap()[0].event_id;
        let mut app = App::new(Some(&s.id));
        app.refresh(&store).unwrap();
        app.switch_view(View::Artifacts);
        (root, store, app, base)
    }
    #[test]
    fn editor_saves_new_version_and_selected_version_comment_through_real_actions() {
        let (root, store, mut app, base) = setup();
        let editor = root.path().join("fixture-editor");
        std::fs::write(&editor, "#!/bin/sh\nprintf 'user revised' > \"$1\"\n").unwrap();
        std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(app.more_actions().contains(&shell::Action::EditArtifact));
        app.on_key(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        let job = app.pending_artifact_review.take().unwrap();
        let draft_path = job.directory.path().join("draft.md");
        assert_eq!(
            std::fs::metadata(&draft_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let edited = job.edit_with(editor.to_str().unwrap());
        app.finish_artifact_review(&store, job, edited);
        assert!(app.notice.contains("New user-authored"));
        assert!(!draft_path.exists());
        let session = app.session().unwrap().clone();
        assert_eq!(
            store
                .artifact_version(&session, "plan.md", base)
                .unwrap()
                .content,
            "original"
        );
        app.open_artifact_mode(&store, crate::artifact_history::Mode::Content)
            .unwrap();
        let browser = app.artifact_browser.as_mut().unwrap();
        browser.index = browser
            .versions
            .iter()
            .position(|v| v.event_id == base)
            .unwrap();
        browser.load(&store, &session);
        app.activate(shell::Action::CommentArtifact, root.path(), &store)
            .unwrap();
        let job = app.pending_artifact_review.take().unwrap();
        app.finish_artifact_review(&store, job, Ok("note about original".into()));
        app.activate(shell::Action::ArtifactComments, root.path(), &store)
            .unwrap();
        assert!(app
            .artifact_browser
            .as_ref()
            .unwrap()
            .text
            .contains("note about original"));
        let latest = store.artifact_versions(&session, "plan.md").unwrap()[0].event_id;
        assert!(!store
            .artifact_comments(&session, "plan.md", latest)
            .unwrap()
            .contains("note about original"));
    }
    #[test]
    fn unchanged_draft_saves_nothing_and_conflict_retains_private_recovery_copy() {
        let (_root, store, mut app, base) = setup();
        app.prepare_artifact_review(&store, false).unwrap();
        let job = app.pending_artifact_review.take().unwrap();
        app.finish_artifact_review(&store, job, Ok("original".into()));
        assert!(app.notice.contains("nothing saved"));
        app.prepare_artifact_review(&store, false).unwrap();
        let job = app.pending_artifact_review.take().unwrap();
        let path = job.directory.path().join("draft.md");
        std::fs::write(&path, "my draft").unwrap();
        let session = app.session().unwrap().clone();
        store
            .add_artifact(&session, "plan", "plan.md", "other revision")
            .unwrap();
        app.finish_artifact_review(&store, job, Ok("my draft".into()));
        assert!(app
            .diagnostic
            .as_ref()
            .unwrap()
            .contains("Latest artifact version changed"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "my draft");
        assert_eq!(
            store.artifact_versions(&session, "plan.md").unwrap().len(),
            2
        );
        assert_eq!(
            store
                .artifact_version(&session, "plan.md", base)
                .unwrap()
                .content,
            "original"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn replaced_editor_file_cannot_follow_symlinks_and_failed_editor_cannot_import() {
        let (root, store, mut app, _) = setup();
        app.prepare_artifact_review(&store, false).unwrap();
        let job = app.pending_artifact_review.take().unwrap();
        let script = root.path().join("failed-editor");
        std::fs::write(&script, "#!/bin/sh\nprintf 'draft' > \"$1\"\nexit 1\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(job.edit_with(script.to_str().unwrap()).is_err());
        let path = job.directory.path().join("draft.md");
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(root.path().join("outside"), &path).unwrap();
        std::fs::write(root.path().join("outside"), "secret").unwrap();
        assert!(job.edit_with("/usr/bin/true").is_err());
        assert_eq!(
            store
                .artifact_versions(app.session().unwrap(), "plan.md")
                .unwrap()
                .len(),
            1
        );
    }
}
