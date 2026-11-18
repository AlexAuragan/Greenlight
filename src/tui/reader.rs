//! Bounded background preparation of large document viewports.
//! The worker owns parsing/wrapping; the UI only paints a ready viewport.
use super::*;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use std::{
    cell::RefCell,
    sync::{
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc,
    },
};

const INLINE_BYTES: usize = 8192;
const INLINE_LINES: usize = 64;

pub(super) struct Document<'a> {
    pub tag: String,
    pub source: &'a str,
    pub markdown: bool,
    pub headers: Vec<Line<'static>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameKey {
    revision: u64,
    width: u16,
    height: u16,
    scroll: u16,
}
struct Job {
    diff: bool,
    key: FrameKey,
    source: Arc<str>,
    markdown: bool,
    headers: Vec<Line<'static>>,
}
struct Prepared {
    key: FrameKey,
    buffer: Buffer,
    offset: u16,
}
struct Worker {
    jobs: SyncSender<Job>,
    results: Receiver<Prepared>,
}
struct CachedDocument {
    revision: u64,
    text: Text<'static>,
    lines: Option<(u16, usize)>,
}
fn prepare(job: Job, cached: &mut Option<CachedDocument>) -> Prepared {
    if cached
        .as_ref()
        .is_none_or(|c| c.revision != job.key.revision)
    {
        let mut text = document_text(&job.source,job.markdown,job.diff);
        text.lines.splice(0..0, job.headers);
        *cached = Some(CachedDocument {
            revision: job.key.revision,
            text,
            lines: None,
        });
    }
    let cached = cached.as_mut().unwrap();
    let paragraph = Paragraph::new(cached.text.clone()).wrap(Wrap { trim: false });
    let lines = match cached.lines {
        Some((width, lines)) if width == job.key.width => lines,
        _ => {
            let lines = paragraph.line_count(job.key.width);
            cached.lines = Some((job.key.width, lines));
            lines
        }
    };
    let maximum = lines
        .saturating_sub(job.key.height as usize)
        .min(u16::MAX as usize) as u16;
    let offset = job.key.scroll.min(maximum);
    let area = Rect::new(0, 0, job.key.width, job.key.height);
    let mut buffer = Buffer::empty(area);
    paragraph.scroll((offset, 0)).render(area, &mut buffer);
    Prepared {
        key: job.key,
        buffer,
        offset,
    }
}
impl Worker {
    fn start() -> std::io::Result<Self> {
        let (jobs, rx) = mpsc::sync_channel::<Job>(1);
        let (tx, results) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("greenlight-preview".into())
            .spawn(move || {
                let mut cached: Option<CachedDocument> = None;
                while let Ok(job) = rx.recv() {
                    if tx.send(prepare(job, &mut cached)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self { jobs, results })
    }
}
#[derive(Default)]
struct State {
    worker: Option<Worker>,
    failed: bool,
    revision: u64,
    tag: String,
    diff: bool,
    source: Option<Arc<str>>,
    markdown: bool,
    headers: Vec<Line<'static>>,
    sent: Option<FrameKey>,
    ready: Option<Prepared>,
}
#[derive(Default)]
pub(super) struct Reader {
    state: RefCell<State>,
}

fn prefix(source: &str) -> &str {
    // Keep one complete character beyond the renderer's byte limit so that its
    // truncation notice is retained without cloning an unlimited stored body.
    let mut end = source.len().min(crate::markdown::MAX_BYTES + 4);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    &source[..end]
}

pub(super) fn draw_document(
    frame: &mut Frame,
    app: &App,
    document: Document<'_>,
    block: Block<'static>,
    area: Rect,
) {
    draw_formatted(frame,app,document,block,area,false);
}
pub(super) fn draw_diff_document(frame:&mut Frame,app:&App,document:Document<'_>,block:Block<'static>,area:Rect) {
    draw_formatted(frame,app,document,block,area,true);
}
fn document_text(source:&str,markdown:bool,diff:bool)->Text<'static> {
    let mut text=if markdown {crate::markdown::render(source)}else{crate::markdown::raw(source)};
    if diff {
        let mut in_diff=false;
        for line in &mut text.lines {
            let content=line.to_string();
            if content.starts_with("--- ") {in_diff=true;}
            if !in_diff {continue}
            line.style=Style::default().fg(if content.starts_with("--- ") || content.starts_with("+++ "){theme::MUTED}else if content.starts_with('+'){theme::GOOD}else if content.starts_with('-'){theme::ERROR}else if content.starts_with("@@"){theme::FOCUS}else{theme::TEXT});
        }
    }
    text
}
fn draw_formatted(frame:&mut Frame,app:&App,document:Document<'_>,block:Block<'static>,area:Rect,diff:bool) {
    if document.source.len() <= INLINE_BYTES
        && document.source.split('\n').take(INLINE_LINES + 1).count() <= INLINE_LINES
    {
        let mut text = document_text(document.source,document.markdown,diff);
        text.lines.splice(0..0, document.headers);
        draw_scrolled(
            frame,
            app,
            Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
            area,
        );
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let source = prefix(document.source);
    let mut state = app.reader.state.borrow_mut();
    if state.tag != document.tag
        || state.diff != diff
        || state.markdown != document.markdown
        || state.source.as_deref() != Some(source)
        || state.headers != document.headers
    {
        state.revision = state.revision.wrapping_add(1);
        state.tag = document.tag;
        state.markdown = document.markdown;
        state.diff = diff;
        state.source = Some(Arc::from(source));
        state.headers = document.headers;
        state.sent = None;
        state.ready = None;
    }
    let key = FrameKey {
        revision: state.revision,
        width: inner.width,
        height: inner.height,
        scroll: app.detail_scroll,
    };
    if state.worker.is_none() && !state.failed {
        match Worker::start() {
            Ok(worker) => state.worker = Some(worker),
            Err(_) => state.failed = true,
        }
    }
    let mut completed = Vec::new();
    if let Some(worker) = &state.worker {
        loop {
            match worker.results.try_recv() {
                Ok(result) => completed.push(result),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    state.failed = true;
                    break;
                }
            }
        }
    }
    for result in completed {
        if state.sent == Some(result.key) {
            state.sent = None;
        }
        if result.key == key {
            state.ready = Some(result);
        }
    }
    if let Some(ready) = state.ready.as_ref().filter(|r| r.key == key) {
        app.rendered_scroll
            .set(Some((app.detail_scroll, ready.offset)));
        let target = frame.buffer_mut();
        for y in 0..inner.height {
            for x in 0..inner.width {
                target[(inner.x + x, inner.y + y)] = ready.buffer[(x, y)].clone();
            }
        }
        return;
    }
    app.rendered_scroll.set(None);
    if state.sent != Some(key) && !state.failed {
        let job = Job {
            diff: state.diff,
            key,
            source: state.source.as_ref().unwrap().clone(),
            markdown: state.markdown,
            headers: state.headers.clone(),
        };
        match state.worker.as_ref().unwrap().jobs.try_send(job) {
            Ok(()) => state.sent = Some(key),
            Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => state.failed = true,
        }
    }
    let message = if state.failed {
        "Document preview unavailable; use the CLI to export the source."
    } else {
        "Preparing document preview…"
    };
    frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    fn manual_worker(app: &App) -> (Receiver<Job>, SyncSender<Prepared>) {
        let (jobs, rx) = mpsc::sync_channel(1);
        let (tx, results) = mpsc::sync_channel(1);
        app.reader.state.borrow_mut().worker = Some(Worker { jobs, results });
        (rx, tx)
    }
    fn screen(app: &App, source: &str, markdown: bool, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
        terminal
            .draw(|frame| {
                draw_document(
                    frame,
                    app,
                    Document {
                        tag: "document".into(),
                        source,
                        markdown,
                        headers: Vec::new(),
                    },
                    Block::bordered(),
                    frame.area(),
                )
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }
    #[test]
    fn pending_work_does_not_block_keyboard_routing_or_queue_duplicate_jobs() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        let (jobs, results) = manual_worker(&app);
        let source = format!("# READY\n{}", "body\n".repeat(2000));
        assert!(screen(&app, &source, true, 80).contains("Preparing document preview"));
        app.on_key(
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        app.on_key(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.composer, "x");
        assert_eq!(app.focus, Focus::Compose);
        screen(&app, &source, true, 80);
        let job = jobs.try_recv().unwrap();
        assert!(jobs.try_recv().is_err());
        results.send(prepare(job, &mut None)).unwrap();
        assert!(screen(&app, &source, true, 80).contains("READY"));
        screen(&app, &source, true, 80);
        assert!(
            jobs.try_recv().is_err(),
            "cached viewport must not reschedule parsing"
        );
    }
    #[test]
    fn stale_source_mode_and_size_results_are_never_painted() {
        let app = App::new(None);
        let (jobs, results) = manual_worker(&app);
        let old = format!("# OLD\n{}", "old\n".repeat(3000));
        let new = format!("# NEW\n{}", "new\n".repeat(3000));
        screen(&app, &old, true, 80);
        let stale = jobs.try_recv().unwrap();
        screen(&app, &new, false, 60);
        let current = jobs.try_recv().unwrap();
        results.send(prepare(stale, &mut None)).unwrap();
        let output = screen(&app, &new, false, 60);
        assert!(!output.contains("OLD"));
        assert!(output.contains("Preparing document preview"));
        results.send(prepare(current, &mut None)).unwrap();
        assert!(screen(&app, &new, false, 60).contains("# NEW"));
        assert!(screen(&app, &new, true, 80).contains("Preparing document preview"));
        results
            .send(prepare(jobs.try_recv().unwrap(), &mut None))
            .unwrap();
        let output = screen(&app, &new, true, 80);
        assert!(output.contains("▍ NEW"));
        assert!(!output.contains("# NEW"));
    }
    #[test]
    fn worker_matches_synchronous_wrapping_styles_and_scroll_bounds() {
        let source = format!(
            "# 漢字 **Heading**\n{}\n--- a/file\n+++ b/file\n@@ hunk @@\n-old\n+new\nTAIL",
            "e\u{301} 👩‍💻 `code` **bold**\n".repeat(500)
        );
        for (markdown,diff) in [(true,false),(false,false),(false,true)] {
            for width in [20, 60, 100] {
                let job = Job {
                    diff,
                    key: FrameKey {
                        revision: 1,
                        width,
                        height: 12,
                        scroll: u16::MAX,
                    },
                    source: Arc::from(source.as_str()),
                    markdown,
                    headers: vec![Line::raw("Header")],
                };
                let result = prepare(job, &mut None);
                let mut text = document_text(&source,markdown,diff);
                text.lines.insert(0, Line::raw("Header"));
                let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
                let maximum = paragraph
                    .line_count(width)
                    .saturating_sub(12)
                    .min(u16::MAX as usize) as u16;
                let area = Rect::new(0, 0, width, 12);
                let mut expected = Buffer::empty(area);
                paragraph.scroll((maximum, 0)).render(area, &mut expected);
                assert_eq!(result.buffer, expected);
                assert_eq!(result.offset, maximum);
            }
        }
    }
    #[test]
    fn source_copy_is_bounded_and_truncation_is_preserved() {
        let source = "é".repeat(crate::markdown::MAX_BYTES);
        let bounded = prefix(&source);
        assert!(bounded.len() <= crate::markdown::MAX_BYTES + 4);
        assert!(bounded.len() > crate::markdown::MAX_BYTES);
        let result = prepare(
            Job {
                diff: false,
                key: FrameKey {
                    revision: 1,
                    width: 80,
                    height: 12,
                    scroll: u16::MAX,
                },
                source: Arc::from(bounded),
                markdown: true,
                headers: Vec::new(),
            },
            &mut None,
        );
        let text = result
            .buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("Preview truncated"));
    }
    #[test]
    fn disconnected_worker_shows_error_without_blocking_or_showing_old_content() {
        let app = App::new(None);
        let (jobs, results) = manual_worker(&app);
        drop(jobs);
        drop(results);
        let source = "large document\n".repeat(1000);
        assert!(screen(&app, &source, true, 80).contains("Document preview unavailable"));
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn returning_to_a_discarded_viewport_reschedules_instead_of_waiting_forever() {
        let app = App::new(None);
        let (jobs, rx) = mpsc::sync_channel(1);
        let (tx, results) = mpsc::sync_channel(1);
        app.reader.state.borrow_mut().worker = Some(Worker { jobs, results });
        let source = "RENDER ME\n".repeat(1000);
        let draw = |width| {
            let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
            terminal
                .draw(|f| {
                    draw_document(
                        f,
                        &app,
                        Document {
                            tag: "same".into(),
                            source: &source,
                            markdown: false,
                            headers: Vec::new(),
                        },
                        Block::bordered(),
                        f.area(),
                    )
                })
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };
        draw(80);
        let wide = rx.try_recv().unwrap();
        draw(60);
        let narrow = rx.try_recv().unwrap();
        tx.send(prepare(wide, &mut None)).unwrap();
        draw(60);
        tx.send(prepare(narrow, &mut None)).unwrap();
        draw(80);
        let again = rx
            .try_recv()
            .expect("discarded completed requests must be eligible for rescheduling");
        assert_eq!(again.key.width, 78);
        tx.send(prepare(again, &mut None)).unwrap();
        assert!(draw(80).contains("RENDER ME"));
    }
}

#[cfg(test)]
mod approval_priority_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn a_completed_document_cannot_replace_an_approval_or_change_its_keys() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        let (jobs, rx) = mpsc::sync_channel(1);
        let (tx, results) = mpsc::sync_channel(1);
        app.reader.state.borrow_mut().worker = Some(Worker { jobs, results });
        let source = "OLD PREVIEW\n".repeat(1000);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| {
                draw_document(
                    f,
                    &app,
                    Document {
                        tag: "old".into(),
                        source: &source,
                        markdown: true,
                        headers: Vec::new(),
                    },
                    Block::bordered(),
                    f.area(),
                )
            })
            .unwrap();
        tx.send(prepare(rx.try_recv().unwrap(), &mut None)).unwrap();
        let (_pt, prompts) = mpsc::channel();
        let (answers, received) = mpsc::channel();
        let (_ft, finished) = mpsc::channel();
        app.runner = Some(Runner { live:None,
            cancel_retry: None,
            session_id: None,
            prompts,
            answers,
            finished,
        });
        app.approval = Some(ApprovalPrompt { review: None, command: None,
            mode: ApprovalMode::Human,
            content: "Review this approval".into(),
        });
        terminal.draw(|f| super::super::draw(f, &app)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(output.contains("Review this approval"));
        assert!(!output.contains("OLD PREVIEW"));
        app.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert!(received.try_recv().is_err());
        app.on_key(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(received.try_recv().unwrap(), "y");
    }
}

#[cfg(test)]
mod line_budget_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn many_short_table_rows_use_the_worker_even_below_the_byte_threshold() {
        let app = App::new(None);
        let (jobs, rx) = mpsc::sync_channel(1);
        let (_tx, results) = mpsc::sync_channel(1);
        app.reader.state.borrow_mut().worker = Some(Worker { jobs, results });
        let source = format!(
            "| {} | {} |\n| --- | --- |\n{}",
            "Wide header".repeat(5),
            "Wide header".repeat(5),
            "| a | b |\n".repeat(100)
        );
        assert!(source.len() < INLINE_BYTES);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| {
                draw_document(
                    f,
                    &app,
                    Document {
                        tag: "sparse table".into(),
                        source: &source,
                        markdown: true,
                        headers: Vec::new(),
                    },
                    Block::bordered(),
                    f.area(),
                )
            })
            .unwrap();
        assert!(
            rx.try_recv().is_ok(),
            "padding amplification must not run on the input thread"
        );
    }
}
