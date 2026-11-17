//! Actual Ratatui render fixtures, also exportable for visual review.
use super::*;
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
fn fixture() -> (tempfile::TempDir, Store, Session, App, i64) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut session = store
        .create(
            "optimize-performance",
            "The bars take about 30s to display; full goal belongs in Details, not the header",
            "evaluate",
        )
        .unwrap();
    for name in [
        "baseline.md",
        "baseline_draft_notes.md",
        "benchmark.md",
        "benchmark_measurement_notes.md",
        "checks_availability_note.md",
        "checks_to_run.md",
        "investigations_status.md",
        "logs_request.md",
        "optimization.md",
        "optimization_proposal.md",
        "read_availability_note.md",
    ] {
        store
            .add_artifact(
                &session,
                "baseline",
                name,
                "# Saved evidence\n\nModel-authored notes.",
            )
            .unwrap();
    }
    store
        .event(&session, Some("evaluate"), "step.started", "")
        .unwrap();
    store.add_artifact(&session,"evaluate","performance-evaluation.md","# Performance evaluation\n\nDO NOT DUMP THIS REPORT INTO THE OVERVIEW.\n\nMeasurement was unavailable; no measured improvement established.").unwrap();
    let report = store
        .artifact_versions(&session, "performance-evaluation.md")
        .unwrap()[0]
        .event_id;
    store
        .event(&session, Some("evaluate"), "verification.none", "")
        .unwrap();
    store
        .event(&session, Some("evaluate"), "verification.passed", "")
        .unwrap();
    store.event(&session,Some("evaluate"),"step.completed","Performance evaluation recorded. Status: measurement_failure (benchmark unavailable). No numeric before/after pair exists. The optimization remains unverified and unimplemented pending source listings and logs. Further loops cannot establish an improvement without new measured evidence. Ask the user for the required logs before another benchmark attempt.").unwrap();
    store
        .event(&session, None, "session.completed", "")
        .unwrap();
    session.status = "completed".into();
    store.save(&session).unwrap();
    let mut app = App::new(Some(&session.id));
    app.shell.project = "bar-project".into();
    app.refresh(&store).unwrap();
    (root, store, session, app, report)
}
fn screen(app: &mut App, w: u16, h: u16, name: &str) -> Buffer {
    app.viewport = (w, h);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| view::draw(f, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    if let Ok(dir) = std::env::var("GREENLIGHT_CAPTURE_DIR") {
        if name.starts_with("focus-") && (name == "focus-cycle" || (w, h) != (80, 24)) {
            return buffer;
        }
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        fn color(c: Color) -> String {
            match c {
                Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
                Color::Black => "#000000".into(),
                Color::White => "#ffffff".into(),
                Color::Cyan | Color::LightCyan => "#67e8f9".into(),
                Color::DarkGray => "#94a3b8".into(),
                Color::Green | Color::LightGreen => "#4ade80".into(),
                Color::Yellow => "#fbbf24".into(),
                Color::Red | Color::LightRed => "#f87171".into(),
                Color::Blue | Color::LightBlue => "#93c5fd".into(),
                _ => "#e5e7eb".into(),
            }
        }
        let cells=buffer.content.iter().map(|c|serde_json::json!({"text":c.symbol(),"fg":color(c.fg),"bg":if c.bg==Color::Reset {"#111827".to_owned()}else{color(c.bg)},"bold":c.modifier.contains(Modifier::BOLD)})).collect::<Vec<_>>();
        std::fs::write(
            dir.join(format!("{name}-{w}x{h}.json")),
            serde_json::to_vec(&serde_json::json!({"width":w,"height":h,"cells":cells})).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join(format!("{name}-{w}x{h}.txt")), text(&buffer)).unwrap();
    }
    buffer
}
fn text(buffer: &Buffer) -> String {
    buffer
        .content
        .chunks(buffer.area.width.max(1) as usize)
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn result_overview_is_scoped_compact_and_opens_exact_report() {
    let (root, store, session, mut app, report) = fixture();
    store
        .add_artifact(
            &session,
            "evaluate",
            "performance-evaluation.md",
            "NEWER UNRELATED REPORT",
        )
        .unwrap();
    app.refresh(&store).unwrap();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let buffer = screen(&mut app, w, h, "result");
        let screen = text(&buffer);
        assert!(!screen.contains("DO NOT DUMP"));
        assert!(!screen.contains("NEWER UNRELATED"));
        assert!(!screen.contains("The bars take"));
        assert!(!screen.contains("event #"));
        assert!(!screen.contains("Workflow result ·"));
        assert!(screen.contains("Agent-reported conclusion"));
        if w >= 80 {
            assert!(screen.contains("Recorded verification"));
            assert!(screen.contains("Recorded native edits"));
            assert!(screen.contains("No checks required"));
            assert!(screen.contains("Read final report"));
            assert!(!screen.contains("Open selected"));
        }
        assert_eq!(buffer[(0, 0)].bg, theme::BACKGROUND);
        if (w, h) == (80, 24) {
            assert_eq!(screen, include_str!("../docs/screenshots/result-80x24.txt"));
            assert_eq!(buffer[(1, 7)].bg, theme::PANEL);
        }
    }
    app.activate(shell::Action::Report, root.path(), &store)
        .unwrap();
    let browser = app.artifact_browser.as_ref().unwrap();
    assert_eq!(browser.versions[browser.index].event_id, report);
    assert!(browser.text.contains("DO NOT DUMP"));
    screen(&mut app, 80, 24, "report-reader");
}
#[test]
fn artifacts_are_single_rows_with_contextual_actions_and_explicit_reading() {
    let (root, store, _, mut app, _) = fixture();
    app.switch_view(View::Artifacts);
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "artifacts"));
        assert!(output.contains("Artifacts · 12 saved"));
        assert!(!output.contains("step:"));
        assert!(!output.contains("v versions"));
        assert!(!output.contains("[Open selected]"));
        if w >= 80 {
            assert!(output.contains("performance-evaluation.md"));
            assert!(output.contains("Versions"));
        }
        assert!(!output.contains("Saved evidence"));
        if (w, h) == (80, 24) {
            assert_eq!(
                output,
                include_str!("../docs/screenshots/artifacts-80x24.txt")
            );
        }
    }
    app.activate(shell::Action::Open, root.path(), &store)
        .unwrap();
    assert_eq!(app.focus, Focus::Detail);
    assert!(text(&screen(&mut app, 80, 24, "artifact-reader")).contains("Saved evidence"));
}
#[test]
fn context_actions_and_header_actions_remain_keyboard_reachable() {
    let (root, store, _, mut app, _) = fixture();
    for wanted in [
        shell::Action::Report,
        shell::Action::ReviewEdits,
        shell::Action::More,
        shell::Action::Sessions,
        shell::Action::Details,
        shell::Action::Help,
    ] {
        let actions = app.shell_actions();
        assert!(actions.contains(&wanted));
    }
    app.on_key(
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(
        app.shell_actions()[app.shell.action_index],
        shell::Action::Report
    );
    app.on_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(app.artifact_browser.is_some());
    app.on_key(
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    app.on_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(!app.artifact_browser.as_ref().unwrap().reading);
}
#[test]
fn historical_results_and_unknown_verdicts_do_not_claim_success() {
    let (_root, store, session, mut app, _) = fixture();
    store
        .event(&session, None, "workspace.restored", "source.rs")
        .unwrap();
    app.refresh(&store).unwrap();
    let output = text(&screen(&mut app, 80, 24, "result-restored"));
    assert!(output.contains("Historical result"));
    assert!(output.contains("earlier workspace"));
    let no_verdict = result::overview(&session, &[]);
    assert!(!no_verdict.checked);
    assert!(no_verdict.verification.contains("No recorded"));
}

#[test]
fn every_tab_stop_has_one_visible_focus_marker_and_fill() {
    let (root, store, _, mut app, _) = fixture();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        app.shell.actions_focus = false;
        let initial = screen(&mut app, w, h, "result");
        assert!(
            !initial.content.iter().any(|c| c.bg == theme::FOCUS),
            "primary action must not look focused"
        );
        let actions = app.shell_actions();
        for (index, action) in actions.iter().enumerate() {
            app.on_key(
                KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                root.path(),
                &store,
            )
            .unwrap();
            assert_eq!(app.shell_actions()[app.shell.action_index], *action);
            let name = if *action == shell::Action::Report {
                "focus-report"
            } else if *action == shell::Action::Sessions {
                "focus-sessions"
            } else {
                "focus-cycle"
            };
            let buffer = screen(&mut app, w, h, name);
            let markers = buffer
                .content
                .iter()
                .enumerate()
                .filter(|(_, c)| c.symbol() == "›")
                .collect::<Vec<_>>();
            assert_eq!(
                markers.len(),
                1,
                "exactly one focus marker for {action:?} at {w}x{h}"
            );
            let (position, marker) = markers[0];
            assert_eq!(
                marker.bg,
                theme::FOCUS,
                "focused control needs high-contrast fill"
            );
            assert_eq!(marker.fg, theme::BACKGROUND);
            assert!(marker.modifier.contains(Modifier::BOLD));
            // All filled cells belong to the focused control's visible rows.
            let row = position / w as usize;
            assert!(buffer
                .content
                .iter()
                .enumerate()
                .filter(|(_, c)| c.bg == theme::FOCUS)
                .all(|(i, _)| if index < app.footer_actions().len() {
                    (row.saturating_sub(1)..=row + 1).contains(&(i / w as usize))
                } else {
                    i / w as usize == row
                }));
        }
        app.on_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.shell.action_index, 0, "forward wrap");
        app.on_key(
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.shell.action_index, actions.len() - 1, "reverse wrap");
        app.on_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert!(!screen(&mut app, w, h, "result")
            .content
            .iter()
            .any(|c| c.bg == theme::FOCUS));
        // An Activity entry in More is a focused action, not a selected tab.
        app.shell.more = true;
        app.shell.menu_index = app
            .more_actions()
            .iter()
            .position(|a| *a == shell::Action::Activity)
            .unwrap();
        let menu = screen(&mut app, w, h, "focus-cycle");
        assert_eq!(
            menu.content
                .iter()
                .filter(|c| c.symbol() == "›" && c.bg == theme::FOCUS)
                .count(),
            1
        );
        app.shell.more = false;
    }
}
