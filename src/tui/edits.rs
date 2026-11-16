//! Branch-visible native edit review and guarded, explicit workspace-only undo.
use super::*;
use std::collections::BTreeMap;

pub(super) struct File {
    pub path: String,
    before: Option<String>,
    after: Option<String>,
    pub gap: bool,
    pub writes: usize,
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
#[derive(Default)]
pub(super) struct Edits {
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
        if self.generation.as_ref() == Some(&generation) {
            return Ok(());
        }
        let selected = self.files.get(self.index).map(|f| f.path.clone());
        let mut files: BTreeMap<String, File> = BTreeMap::new();
        for edit in store.workspace_edits(session)? {
            let file = files.entry(edit.path.clone()).or_insert_with(|| File {
                path: edit.path,
                before: edit.before.clone(),
                after: edit.before.clone(),
                gap: false,
                writes: 0,
            });
            file.gap |= file.after != edit.before;
            file.after = edit.after;
            file.writes += 1;
        }
        if self
            .generation
            .as_ref()
            .is_some_and(|g| g.0 != session.id || g.1 != session.active_branch)
        {
            self.index = 0;
        }
        self.files = files.into_values().collect();
        self.index = selected
            .and_then(|path| self.files.iter().position(|f| f.path == path))
            .unwrap_or(self.index)
            .min(self.files.len().saturating_sub(1));
        self.generation = Some(generation);
        self.error = None;
        self.select(0);
        Ok(())
    }
    pub fn select(&mut self, delta: isize) {
        self.index = move_index(self.index, delta, self.files.len());
        self.text=match self.files.get(self.index) {
            None=>"No recorded native edits on this branch.\n\nArtifact documents are not source edits. Shell/manual changes are outside this journal.".into(),
            Some(file)=>{
                let mut diff=changes::unified(&file.path,file.before.as_deref(),file.after.as_deref().unwrap_or(""));
                if file.after.is_none(){diff=diff.replacen(&format!("+++ b/{}\n",file.path),"+++ /dev/null\n",1);}
                format!("{} · {} · {} recorded change(s)\n\nNet recorded diff: before first tracked edit → latest recorded state.\nManual drift is checked before any revert.{}\n\n{}",file.path,file.label(),file.writes,if file.gap {"\nHistory has an out-of-band gap; automatic revert is unavailable."} else {""},diff)
            }
        };
    }
}
impl App {
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
            let preview=format!("Revert {label}\n\nRestore the selected files to their first recorded pre-edit contents.\nThis changes workspace files only; workflow history and artifacts remain historical.\nNo workflow is resumed or forked.\n\n{}\ny confirms · n/Esc cancels · Enter does nothing",plan.preview_text());
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
                self.edits.pending = Some(pending);
                self.detail_scroll = 0;
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
        draw_scrolled(
            frame,
            app,
            Paragraph::new(safe_display(&pending.preview))
                .wrap(Wrap { trim: false })
                .block(border(" Revert preview · y confirm / n cancel ", true)),
            area,
        );
        return;
    }
    let panels =
        Layout::horizontal([Constraint::Percentage(33), Constraint::Percentage(67)]).split(area);
    let items = app.edits.files.iter().map(|f| {
        ListItem::new(vec![
            Line::from(inline_display(&f.path)),
            Line::styled(
                f.label(),
                Style::default().fg(if f.gap {
                    Color::Red
                } else if f.changed() {
                    Color::Yellow
                } else {
                    Color::DarkGray
                }),
            ),
        ])
    });
    let mut selection = ListState::default();
    if !app.edits.files.is_empty() {
        selection.select(Some(app.edits.index));
    }
    frame.render_stateful_widget(
        List::new(items)
            .block(border(
                " Files · u revert · U all ",
                app.focus == Focus::Artifacts,
            ))
            .highlight_symbol("▶ ")
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan)),
        panels[0],
        &mut selection,
    );
    let source = app.edits.error.as_deref().unwrap_or(&app.edits.text);
    reader::draw_document(
        frame,
        app,
        reader::Document {
            tag: format!("edits:{:?}:{}", app.edits.generation, app.edits.index),
            source,
            markdown: false,
            headers: Vec::new(),
        },
        border(
            " Recorded diff · Enter read/list ",
            app.focus == Focus::Detail,
        ),
        panels[1],
    );
}
