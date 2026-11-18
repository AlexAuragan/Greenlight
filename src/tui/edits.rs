//! Branch-visible native edit review and guarded, explicit workspace-only undo.
use super::*;
use std::collections::BTreeMap;

pub(super) struct File {
    pub path: String,
    before: Option<String>,
    after: Option<String>,
    pub gap: bool,
    pub writes: usize,
    pub revision: i64,
    pub kept: bool,
}
impl File {
    pub fn changed(&self) -> bool {
        self.before != self.after
    }
    pub fn label(&self) -> &str {
        if self.gap {
            "history gap"
        } else if !self.changed() {
            "no net change / reverted"
        } else if self.before.is_none() {
            "added"
        } else if self.after.is_none() {
            "deleted"
        } else {
            "modified"
        }
    }
}
pub(super) struct Pending {
    plan: RestorePlan,
    pub preview: String,
    session: String,
    branch: String,
    cutoff: Option<i64>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    path: String,
    event: i64,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    session: String,
    branch: String,
    files: Vec<Revision>,
}
#[derive(Default)]
pub(super) struct Edits {
    pub overview: bool,
    reload: bool,
    pub files: Vec<File>,
    pub index: usize,
    pub text: String,
    pub error: Option<String>,
    pub pending: Option<Pending>,
    generation: Option<(String, String, Option<i64>)>,
}
impl Edits {
    pub fn load(&mut self, store: &Store, session: &Session) -> Result<()> {
        let history = store.history(session)?;
        let generation = (
            session.id.clone(),
            session.active_branch.clone(),
            history.last().map(|e| e.0),
        );
        if !self.reload && self.generation.as_ref() == Some(&generation) {
            return Ok(());
        }
        let scope_changed = self
            .generation
            .as_ref()
            .is_none_or(|g| g.0 != session.id || g.1 != session.active_branch);
        let selected = if scope_changed {
            None
        } else {
            self.files.get(self.index).map(|f| f.path.clone())
        };
        let mut files: BTreeMap<String, File> = BTreeMap::new();
        for edit in store.workspace_edits(session)? {
            let file = files.entry(edit.path.clone()).or_insert_with(|| File {
                path: edit.path,
                before: edit.before.clone(),
                after: edit.before.clone(),
                gap: false,
                writes: 0,
                revision: edit.event_id,
                kept: false,
            });
            file.gap |= file.after != edit.before;
            file.after = edit.after;
            file.writes += 1;
            file.revision = edit.event_id;
        }
        for (_, _, kind, raw) in &history {
            if kind != "edits.kept" {
                continue;
            }
            let review: Review =
                serde_json::from_str(raw).context("Invalid saved edit review marker")?;
            if review.session != session.id || review.branch != session.active_branch {
                continue;
            }
            for mark in review.files {
                if let Some(file) = files.get_mut(&mark.path) {
                    if file.revision == mark.event {
                        file.kept = true;
                    }
                }
            }
        }
        if scope_changed {
            self.index = 0;
            self.overview = files.values().filter(|f| f.changed() || f.gap).count() != 1;
        }
        self.files = files.into_values().collect();
        self.index = selected
            .and_then(|path| self.files.iter().position(|f| f.path == path))
            .unwrap_or(self.index)
            .min(self.files.len().saturating_sub(1));
        self.generation = Some(generation);
        self.reload=false;
        self.error = None;
        self.select(0);
        Ok(())
    }
    pub fn cycle(&mut self, reverse: bool) {
        let mut views = Vec::new();
        if self.files.len() != 1 || !self.files[0].changed() {
            views.push(None);
        }
        views.extend(
            self.files
                .iter()
                .enumerate()
                .filter(|(_, f)| f.changed() || f.gap)
                .map(|(i, _)| Some(i)),
        );
        if views.is_empty() {
            self.overview = true;
            return;
        }
        let current = if self.overview {
            None
        } else {
            Some(self.index)
        };
        let next = views
            .iter()
            .position(|v| *v == current)
            .map(|position| (position + if reverse { views.len() - 1 } else { 1 }) % views.len())
            .unwrap_or(0);
        self.overview = views[next].is_none();
        if let Some(i) = views[next] {
            self.index = i;
        }
        self.select(0);
    }
    pub fn select(&mut self, delta: isize) {
        self.index = move_index(self.index, delta, self.files.len());
        self.text=match self.files.get(self.index) {
            None=>"No recorded native edits on this branch.\n\nArtifact documents are not source edits. Shell/manual changes are outside this journal.".into(),
            Some(file)=>{
                let mut diff=changes::unified(&file.path,file.before.as_deref(),file.after.as_deref().unwrap_or(""));
                if file.after.is_none(){diff=diff.replacen(&format!("+++ b/{}\n",file.path),"+++ /dev/null\n",1);}
                format!("Net recorded diff: first tracked state → latest recorded state.\n{}\n{}",if file.gap {"History gap: Keep and automatic revert are unavailable."}else{""},diff)
            }
        };
    }
}
impl App {
    pub(super) fn keep_edits(&mut self, root: &Path, store: &Store, all: bool) {
        let result = (|| -> Result<usize> {
            if self.runner.is_some() || self.edits.pending.is_some() {
                bail!("Stop the worker and close any revert preview before recording Keep.");
            }
            let selected = self.session().context("No session selected")?;
            let session = store.session(&selected.id)?;
            let _owner = store.lock_idle_session(&session.id)?;
            let _writes = store.native_write_lock()?;
            store.ensure_no_pending_writes()?;
            let history = store.history(&session)?;
            if self.edits.generation.as_ref()
                != Some(&(
                    session.id.clone(),
                    session.active_branch.clone(),
                    history.last().map(|e| e.0),
                ))
            {
                bail!("Session, branch or recorded revision changed; review the refreshed edits first.");
            }
            let mut recorded=Edits::default();
            recorded.load(store,&session)?;
            if recorded.generation!=self.edits.generation {bail!("Recorded revisions changed during Keep; refresh and review again.");}
            let files = if all {
                self.edits
                    .files
                    .iter()
                    .filter(|f| f.changed() || f.gap)
                    .collect::<Vec<_>>()
            } else {
                vec![self
                    .edits
                    .files
                    .get(self.edits.index)
                    .context("No file selected")?]
            };
            if files.is_empty() {
                bail!("No changed recorded files to Keep.");
            }
            for file in &files {
                if !recorded.files.iter().any(|f|f.path==file.path && f.revision==file.revision && f.before==file.before && f.after==file.after && f.gap==file.gap) {bail!("Recorded snapshots changed while reviewing {}; refresh and review again.",file.path);}
                if file.gap {
                    bail!("History gap for {}; Keep is unavailable.", file.path);
                }
                if !file.changed() {
                    bail!("{} has no net recorded change to Keep.", file.path);
                }
                let path = crate::restore::checked_path(root, &file.path)?;
                let actual = match std::fs::read_to_string(path) {
                    Ok(s) => Some(s),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e.into()),
                };
                if actual != file.after {
                    bail!("{} differs from its latest recorded snapshot; preserve manual/shell changes before reviewing again.",file.path);
                }
            }
            let review = Review {
                session: session.id.clone(),
                branch: session.active_branch.clone(),
                files: files
                    .iter()
                    .map(|f| Revision {
                        path: f.path.clone(),
                        event: f.revision,
                    })
                    .collect(),
            };
            store.event(
                &session,
                None,
                "edits.kept",
                &serde_json::to_string(&review)?,
            )?;
            Ok(files.len())
        })();
        match result {
            Ok(count)=>self.notice=format!("Kept {count} recorded file revision(s). Review saved; no files, permissions or Git state changed."),
            Err(error)=>self.raise_diagnostic(format!("Keep unavailable; no review saved: {error:#}")),
        }
        self.edits.reload=true;
        self.refresh_edits(store);
    }
    pub(super) fn refresh_revert(&mut self, store: &Store) {
        let Some(p) = self.edits.pending.as_ref() else {
            return;
        };
        let result = (|| -> Result<()> {
            let s = store.session(&p.session)?;
            if self.session().map(|s| s.id.as_str()) != Some(p.session.as_str())
                || s.active_branch != p.branch
                || store.history(&s)?.last().map(|e| e.0) != p.cutoff
            {
                bail!("Session, branch or history changed since preview. Prepare a fresh preview.");
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.edits.pending = None;
            self.raise_diagnostic(format!(
                "Revert preview expired; no restore applied: {error:#}"
            ));
        }
    }
    pub(super) fn refresh_edits(&mut self, store: &Store) {
        let Some(session) = self.session().cloned() else {
            self.edits = Edits::default();
            return;
        };
        if let Err(error) = self.edits.load(store, &session) {
            self.edits.files.clear();
            self.edits.text.clear();
            self.edits.error = Some(format!("Recorded edits unavailable: {error:#}"));
        }
    }
    pub(super) fn prepare_revert(&mut self, root: &Path, store: &Store, all: bool) {
        if self.runner.is_some() {
            self.raise_diagnostic("Stop the active worker before reverting files.".into());
            return;
        }
        let result = (|| -> Result<Pending> {
            let selected = self.session().context("No session selected")?;
            let session = store.session(&selected.id)?;
            let _lock = store.lock_idle_session(&session.id)?;
            let history = store.history(&session)?;
            if self.edits.generation.as_ref()
                != Some(&(
                    session.id.clone(),
                    session.active_branch.clone(),
                    history.last().map(|e| e.0),
                ))
            {
                bail!(
                    "Session, branch or history changed; refresh Edits before preparing a revert."
                );
            }
            let event = history
                .iter()
                .find(|e| e.2 == "session.started")
                .context("Initial session checkpoint unavailable; cannot safely revert")?
                .0;
            let selection = if all {
                None
            } else {
                Some(vec![self
                    .edits
                    .files
                    .get(self.edits.index)
                    .context("No file selected")?
                    .path
                    .clone()])
            };
            let plan =
                RestorePlan::prepare_paths(root, store, &session, event, selection.as_deref())?;
            if plan.is_empty() {
                bail!("Selected recorded edits are already reverted; no files need changing.");
            }
            let label = if all {
                "ALL recorded session edits".to_owned()
            } else {
                selection.as_ref().unwrap()[0].clone()
            };
            let reverse=plan.preview_text();
            let reverse=reverse.split_once('\n').map(|(_,body)|body).unwrap_or(&reverse);
            let preview=format!("Revert {label}\nProposed reverse diff · not applied yet\nRestore first recorded pre-edit contents.\nWorkspace only; history/artifacts retained. No fork or automatic resume.\n{}\n\nTechnical scope\nSession: {}\nBranch: {}\ny confirms · n/Esc cancels · Enter does nothing",reverse,session.id,session.active_branch);
            Ok(Pending {
                plan,
                preview,
                session: session.id,
                branch: session.active_branch,
                cutoff: history.last().map(|e| e.0),
            })
        })();
        match result {
            Ok(pending) => {
                self.shell.request_epoch += 1;
                self.edits.pending = Some(pending);
                self.detail_scroll = 0;
                self.notice="Revert preview prepared; no files changed. y confirms · n/Esc cancels.".into();
            }
            Err(error) => self.raise_diagnostic(format!("Revert unavailable: {error:#}")),
        }
    }
    pub(super) fn confirm_revert(&mut self, root: &Path, store: &Store) {
        let Some(pending) = self.edits.pending.take() else {
            return;
        };
        let result = (|| -> Result<()> {
            let session = store.session(&pending.session)?;
            if self.session().map(|s| s.id.as_str()) != Some(&pending.session)
                || session.active_branch != pending.branch
                || store.history(&session)?.last().map(|e| e.0) != pending.cutoff
            {
                bail!("Session/history changed since preview; review a fresh preview.");
            }
            pending.plan.apply_silent(root, store, &session)
        })();
        match result {Ok(())=>self.notice="Recorded edits reverted. History retained; rerun checks against the restored files.".into(),Err(error)=>self.raise_diagnostic(format!("Revert failed: {error:#}. Inspect workspace/history before retrying."))}
        self.detail_scroll = 0;
        self.refresh_edits(store);
    }
}
pub(super) fn draw(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if let Some(pending) = &app.edits.pending {
        reader::draw_diff_document(
            frame,
            app,
            reader::Document {
                tag: format!("revert:{}", app.shell.request_epoch),
                source: &pending.preview,
                markdown: false,
                headers: vec![],
            },
            border(" Revert preview · proposed, not applied ", true),
            area,
        );
        return;
    }
    if let Some(error) = &app.edits.error {
        reader::draw_document(
            frame,
            app,
            reader::Document {
                tag: "edits-error".into(),
                source: error,
                markdown: false,
                headers: vec![],
            },
            Block::default(),
            area,
        );
        return;
    }
    let compact = area.height < 14;
    let panels = Layout::vertical([
        Constraint::Length(if compact { 1 } else { 4 }),
        Constraint::Min(1),
        Constraint::Length(if compact { 0 } else { 2 }),
    ])
    .split(area);
    let changed = app.edits.files.iter().filter(|f| f.changed()).count();
    let kept = app
        .edits
        .files
        .iter()
        .filter(|f| f.changed() && f.kept)
        .count();
    let view = if app.edits.overview {
        "All files".into()
    } else {
        app.edits
            .files
            .get(app.edits.index)
            .map(|f| inline_display(&f.path))
            .unwrap_or_else(|| "No recorded edits".into())
    };
    let status = if app.edits.overview {
        format!(
            "{} changed / {} recorded paths · {} Kept revision(s)",
            changed,
            app.edits.files.len(),
            kept
        )
    } else {
        app.edits
            .files
            .get(app.edits.index)
            .map(|f| {
                format!(
                    "{} · {} · file {} / {}",
                    f.label(),
                    if f.kept { "Kept revision" } else { "Not kept" },
                    app.edits.index + 1,
                    app.edits.files.len()
                )
            })
            .unwrap_or_default()
    };
    let heading = vec![
        Line::styled(
            format!("Edits · {view}"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "Native journal only · shell/manual changes are outside coverage",
            Style::default().fg(theme::MUTED),
        ),
        Line::styled(status, Style::default().fg(theme::MUTED)),
        Line::styled(
            if app.shell.actions_focus {
                "Actions · Esc returns to file review"
            } else {
                "Tab/Shift+Tab cycles views · Enter focuses actions"
            },
            Style::default().fg(theme::FOCUS),
        ),
    ];
    let heading = if compact {
        vec![Line::styled(
            format!(
                "Edits · {view} · {}",
                if app.edits.overview {
                    format!("{changed} changed")
                } else {
                    app.edits
                        .files
                        .get(app.edits.index)
                        .map(|f| if f.kept { "Kept revision" } else { "Not kept" })
                        .unwrap_or("")
                        .into()
                }
            ),
            Style::default().add_modifier(Modifier::BOLD),
        )]
    } else {
        heading
    };
    frame.render_widget(Paragraph::new(heading), panels[0]);
    if app.edits.overview {
        if app.edits.files.is_empty() {
            frame.render_widget(Paragraph::new("No recorded native edits on this branch.\nArtifacts are documents, not source edits.").wrap(Wrap{trim:false}),panels[1]);
        } else {
            let items = app
                .edits
                .files
                .iter()
                .map(|f| {
                    ListItem::new(Line::from(vec![
                        Span::raw(format!(
                            "{}  ",
                            super::view::fit(
                                &f.path,
                                panels[1].width.saturating_sub(30).max(10) as usize
                            )
                        )),
                        Span::styled(
                            format!(
                                "{} · {}",
                                f.label(),
                                if f.kept && f.changed() {
                                    "Kept revision"
                                } else if f.changed() {
                                    "Not kept"
                                } else {
                                    "—"
                                }
                            ),
                            Style::default().fg(if f.gap { theme::ERROR } else { theme::MUTED }),
                        ),
                    ]))
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(app.edits.index));
            frame.render_stateful_widget(
                List::new(items).highlight_symbol("› ").highlight_style(
                    Style::default()
                        .fg(if app.shell.actions_focus {
                            theme::TEXT
                        } else {
                            theme::FOCUS
                        })
                        .bg(theme::PANEL),
                ),
                panels[1],
                &mut state,
            );
            for index in state.offset()
                ..app
                    .edits
                    .files
                    .len()
                    .min(state.offset() + panels[1].height as usize)
            {
                app.shell.hit(
                    ratatui::layout::Rect::new(
                        panels[1].x,
                        panels[1].y + (index - state.offset()) as u16,
                        panels[1].width,
                        1,
                    ),
                    shell::Action::Edit(index),
                );
            }
        }
    } else {
        let source = if compact {
            app.edits
                .text
                .find("\n--- ")
                .map(|i| &app.edits.text[i + 1..])
                .unwrap_or(&app.edits.text)
        } else {
            &app.edits.text
        };
        reader::draw_diff_document(
            frame,
            app,
            reader::Document {
                tag: format!("edits:{:?}:{}", app.edits.generation, app.edits.index),
                source,
                markdown: false,
                headers: vec![],
            },
            Block::default(),
            panels[1],
        );
    }
    frame.render_widget(Paragraph::new("Keep saves review intent, not verification or tool approval.\nKept revision is not a live disk check; files are already applied.").style(Style::default().fg(theme::MUTED)).wrap(Wrap{trim:false}),panels[2]);
}
