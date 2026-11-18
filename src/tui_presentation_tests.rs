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
            if std::env::var_os("GREENLIGHT_UPDATE_CAPTURE_BASELINES").is_none() {
                assert_eq!(screen, include_str!("../docs/screenshots/result-80x24.txt"));
            }
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
        if (w, h) == (80, 24) && std::env::var_os("GREENLIGHT_UPDATE_CAPTURE_BASELINES").is_none() {
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

#[test]
fn creation_and_recipient_editors_render_real_context_and_actions() {
    let root = tempfile::tempdir().unwrap();
    crate::project::init(root.path()).unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    app.shell.project = "bar-project".into();
    app.refresh(&store).unwrap();
    app.new_form(root.path()).unwrap();
    app.new_session.as_mut().unwrap().cursor = app
        .new_session
        .as_ref()
        .unwrap()
        .workflows
        .iter()
        .position(|w| w.id == "optimize-performance")
        .unwrap();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "create-workflow"));
        assert!(output.contains("Choose workflow"));
        assert!(output.contains("Continue") || w == 40);
    }
    app.activate(shell::Action::Continue, root.path(), &store)
        .unwrap();
    app.new_session.as_mut().unwrap().goal = Some(
        "Investigate the bar's startup delay.\nI can supply host logs and rerun measurements."
            .into(),
    );
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "create-goal"));
        assert!(output.contains("Describe the goal"));
        if w >= 80 {
            assert!(output.contains("Start workflow"));
            assert!(output.contains("Clear draft"));
            assert!(output.contains("supply host logs"));
        }
    }
    assert!(store.list().unwrap().is_empty());
    assert!(app.runner.is_none());
    assert!(app.manager_job.is_none());
    let (_root, store, mut session, mut app, _) = fixture();
    app.set_message_mode(compose::Mode::Manager);
    app.focus_composer();
    app.composer = "Where are we, and what is still unverified?".into();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "compose-manager"));
        assert!(output.contains("Ask manager"));
        if w >= 80 {
            assert!(output.contains("Send question"));
            assert!(output.contains("Read-only"));
        }
    }
    session.status = "blocked".into();
    store.save(&session).unwrap();
    app.refresh(&store).unwrap();
    app.set_message_mode(compose::Mode::Step);
    app.focus_composer();
    app.composer = "Here are the host logs I measured manually.\nStartup took 30.1 seconds.".into();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "compose-step"));
        assert!(output.contains("Resume step"));
        if w >= 80 {
            assert!(output.contains("Send & resume"));
            assert!(output.contains("host logs"));
        }
    }
    assert!(app.runner.is_none());
    assert!(app.manager_job.is_none());
    assert_eq!(
        store
            .usage_report(&session, crate::usage::Scope::Session)
            .unwrap()
            .totals
            .requests,
        0
    );
}

#[test]
fn attention_states_render_causes_and_recovery_effects_without_raw_ids() {
    let root = tempfile::tempdir().unwrap();
    crate::project::init(root.path()).unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut session = store
        .create("fix-bug", "Investigate startup latency", "investigate")
        .unwrap();
    store
        .event(&session, Some("investigate"), "step.started", "")
        .unwrap();
    store.event(&session,Some("investigate"),"step.blocked","The required host-service measurement was unavailable. Supply logs or revise the approach before continuing.").unwrap();
    session.status = "blocked".into();
    store.save(&session).unwrap();
    let mut app = App::new(Some(&session.id));
    app.shell.project = "bar-project".into();
    app.refresh(&store).unwrap();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "attention-blocked"));
        assert!(output.contains("Needs attention"));
        assert!(!output.contains("event #"));
        if w >= 80 {
            assert!(output.contains("host-service measurement"));
            assert!(output.contains("Give instructions"));
        }
    }
    let failure = crate::provider_failure::Failure::http(
        401,
        &serde_json::json!({"error":{"message":"fixture-secret-not-for-default-ui"}}),
        None,
    );
    store.event(&session,Some("investigate"),"provider.failure",&serde_json::json!({"call_id":"fixture-call","purpose":"coding","request_number":1,"failure":failure}).to_string()).unwrap();
    store
        .event(
            &session,
            Some("investigate"),
            "step.blocked",
            "Model request failed.",
        )
        .unwrap();
    app.refresh(&store).unwrap();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "attention-provider"));
        assert!(!output.contains("fixture-secret"));
        if w >= 80 {
            assert!(output.contains("credentials rejected"));
            assert!(output
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains("HTTP 401"));
        }
    }
    app.activate(shell::Action::InspectAttention, root.path(), &store)
        .unwrap();
    assert_eq!(app.focus, Focus::Detail);
    app.on_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(app.attention_visible());
    app.activate(shell::Action::FullDiagnostic, root.path(), &store)
        .unwrap();
    screen(&mut app, 80, 24, "attention-evidence");
    app.attention_history();
    let owner = store.claim_execution(&session.id, None).unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(
        app.attention.as_ref().unwrap().kind,
        attention::Kind::Observing
    );
    let output = text(&screen(&mut app, 80, 24, "attention-observing"));
    assert!(output.contains("Observing another process"));
    assert!(!app.footer_actions().contains(&shell::Action::RecoverOwner));
    drop(owner);
    app.refresh(&store).unwrap();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "attention-owner"));
        if w >= 80 {
            assert!(output.contains("Recover owner"));
            assert!(output
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains("No work is resumed"));
        }
    }
    app.activate(shell::Action::RecoverOwner, root.path(), &store)
        .unwrap();
    screen(&mut app, 80, 24, "attention-recovered");
    assert!(app.runner.is_none());
    assert_eq!(store.session(&session.id).unwrap().status, "blocked");
    let mut workspace = app.workspace.clone().unwrap();
    workspace.primary = crate::workspace_view::PrimaryAction::RecoverWrites;
    app.attention = attention::project(
        &session,
        &workspace,
        &store.history(&session).unwrap(),
        false,
        &[],
    );
    app.workspace = Some(workspace);
    app.blocker_card = true;
    app.clear_diagnostic();
    for (w, h) in [(80, 24), (120, 36), (40, 14)] {
        let output = text(&screen(&mut app, w, h, "attention-write"));
        if w >= 80 {
            assert!(output.contains("reconciliation"));
            assert!(output.contains("Recovery guidance"));
            assert!(!app
                .footer_actions()
                .contains(&shell::Action::GiveInstructions));
        }
    }
    assert_eq!(
        store
            .usage_report(&session, crate::usage::Scope::Session)
            .unwrap()
            .totals
            .requests,
        0
    );
}

#[test]
fn run_details_and_usage_render_scoped_summaries_and_exact_request_evidence() {
    let root=tempfile::tempdir().unwrap();
    crate::project::init(root.path()).unwrap();
    let store=Store::open(root.path()).unwrap();
    let mut session=store.create("optimize-performance","Investigate the startup delay and establish a measured improvement.","baseline").unwrap();
    let definition="type: agent\nprompt: Measure startup before optimizing\noutputs:\n  required: []\n";
    store.begin_attempt(&session,"baseline",definition,"fixture-requested").unwrap();
    store.begin_attempt(&session,"baseline",definition,"fixture-requested").unwrap();
    session.status="blocked".into();store.save(&session).unwrap();
    let mut call=crate::usage::Call::new(&session,"baseline","agent","fixture-requested","fixture",crate::usage::ModelMetadata::default(),&[]);
    store.begin_model_call(&session,&mut call).unwrap();
    call.response_model=Some("fixture-returned".into());
    call.status="response-received".into();
    call.counts=serde_json::from_value(serde_json::json!({
        "input":{"value":145271,"source":"provider"},
        "cached_input":{"value":103168,"source":"provider"},
        "output":{"value":10536,"source":"provider"},
        "notes":["Pricing metadata missing; counts do not establish a bill."]
    })).unwrap();
    store.finish_model_call(&session,&call).unwrap();
    let before=store.history(&session).unwrap().len();
    let mut app=App::new(Some(&session.id));app.shell.project="bar-project".into();app.refresh(&store).unwrap();
    app.open_run_details(details::Page::Run,&store);
    for (w,h) in [(80,24),(120,36),(40,14)] {
        let output=text(&screen(&mut app,w,h,"run-details"));
        assert!(output.contains("Run details"));
        assert!(output.contains("Goal"));
        assert!(!output.contains(&session.id));
        assert!(!output.contains(&call.id));
        if w>=80 { assert!(output.contains("Technical details"));assert!(output.contains("Roadmap")); }
    }
    assert_eq!(app.details.text.matches("baseline (").count(),1);
    app.activate(shell::Action::Technical,root.path(),&store).unwrap();
    screen(&mut app,80,24,"run-technical");
    assert!(app.details.text.contains("fixture-requested"));
    assert!(app.details.text.contains("fixture-returned"));
    assert!(app.details.text.contains("Exact recorded definition"));
    app.activate(shell::Action::DetailsBack,root.path(),&store).unwrap();
    app.activate(shell::Action::Usage,root.path(),&store).unwrap();
    for (w,h) in [(80,24),(120,36),(40,14)] {
        let output=text(&screen(&mut app,w,h,"usage-summary"));
        assert!(output.contains("Usage · this session"));
        if w>=80 {
            assert!(output.contains("145271"));assert!(output.contains("103168"));assert!(output.contains("10536"));
            assert!(output.contains("Estimated cost: unavailable"));assert!(output.contains("0 of 1 requests"));
        }
        assert!(!output.contains(&call.id));
    }
    app.activate(shell::Action::Requests,root.path(),&store).unwrap();
    screen(&mut app,80,24,"usage-requests");
    app.on_key(KeyEvent::new(KeyCode::Enter,KeyModifiers::NONE),root.path(),&store).unwrap();
    assert_eq!(app.details.page,Some(details::Page::Request));
    assert_eq!(app.details.selected.as_deref(),Some(call.id.as_str()));
    screen(&mut app,80,24,"usage-request");
    assert!(app.export_snapshot().unwrap().text.contains(&call.id));
    assert_eq!(store.history(&session).unwrap().len(),before);
    assert_eq!(store.usage_report(&session,crate::usage::Scope::Session).unwrap().totals.requests,1);
    assert!(app.runner.is_none());assert!(app.approval.is_none());
    app.details.close();
    let other=store.create("fix-bug","Investigate another issue","investigate").unwrap();
    app.requested_session=Some(other.id);app.refresh(&store).unwrap();
    app.open_run_details(details::Page::Usage,&store);
    screen(&mut app,80,24,"usage-empty");
    assert!(app.details.text.contains("No recorded requests"));
}

#[test]
fn human_checkpoint_and_step_result_have_distinct_review_surfaces() {
    let (root,store,s,mut app,received,_owner)=checkpoint::tests::fixture();
    app.shell.project="sample-project".into();
    let before=store.history(&s).unwrap().len();
    for (w,h) in [(80,24),(120,36),(40,20)] {
        let output=text(&screen(&mut app,w,h,"checkpoint-human"));
        assert!(output.contains("Human checkpoint"));
        assert!(output.contains("Approve"));assert!(output.contains("Decline"));
        assert!(!output.contains("project grant"));assert!(!output.contains("global grant"));
        assert!(!output.contains(&s.id));
        assert!(output.contains("plan.md"));
        if w>=80 {assert!(output.contains("Pinned input documents"));assert!(output.contains("Excerpt only"));}
    }
    let output=text(&screen(&mut app,40,14,"checkpoint-small"));
    assert!(!output.contains("Approve"));
    app.on_key(KeyEvent::new(KeyCode::Char('y'),KeyModifiers::NONE),root.path(),&store).unwrap();
    assert!(received.try_recv().is_err());
    app.viewport=(80,24);
    app.checkpoint_open(&store);
    let output=text(&screen(&mut app,80,24,"checkpoint-evidence"));
    assert!(output.contains("Read-only evidence"));assert!(output.contains("Original exact plan"));
    app.on_key(KeyEvent::new(KeyCode::Char('n'),KeyModifiers::NONE),root.path(),&store).unwrap();
    assert!(received.try_recv().is_err());
    app.checkpoint_back(&store);
    store.add_artifact(&s,"plan","plan.md","A newer unseen plan").unwrap();
    app.refresh_checkpoint(&store);
    let output=text(&screen(&mut app,80,24,"checkpoint-changed"));
    assert!(!output.contains("Approve & continue"));assert!(output.contains("Decline"));assert!(output.contains("newer version"));
    let review=app.approval.as_mut().unwrap().review.as_mut().unwrap();
    review.kind=crate::tools::HumanReviewKind::StepResult;review.manifest=None;review.documents.clear();
    review.question="Approve this step result? Review the available result evidence.".into();
    app.refresh_checkpoint(&store);
    for (w,h) in [(80,24),(120,36),(40,20)] {
        let output=text(&screen(&mut app,w,h,"checkpoint-step-result"));
        assert!(output.contains("Review step result"));assert!(!output.contains("Human checkpoint"));
        assert!(!output.contains("project grant"));
    }
    assert_eq!(store.history(&s).unwrap().len(),before+1);
    assert!(!store.history(&s).unwrap().iter().any(|e|e.2=="human.approved" || e.2=="verification.human"));
}

#[test]
fn edits_review_renders_all_file_keep_and_guarded_revert_states() {
    let root=tempfile::tempdir().unwrap();crate::project::init(root.path()).unwrap();
    let store=Store::open(root.path()).unwrap();let session=store.create("fix-bug","Review source changes","implement").unwrap();
    for (path,before,after) in [("bar.py",Some("start_service()\n"),"start_service_async()\n"),("startup.py",None,"warm_cache()\n"),("unchanged.txt",Some("same\n"),"same\n")] {
        std::fs::write(root.path().join(path),after).unwrap();store.add_change(&session,"implement",path,before,after).unwrap();
    }
    let mut app=App::new(Some(&session.id));app.shell.project="bar-project".into();app.refresh(&store).unwrap();
    app.switch_view(View::Edits);app.refresh_edits(&store);
    for (w,h) in [(80,24),(120,36),(40,14)] {
        let output=text(&screen(&mut app,w,h,"edits-all"));
        assert!(output.contains("All files"));assert!(output.contains("bar.py"));
        if w>=80 {assert!(output.contains("2 changed / 3 recorded paths"));assert!(output.contains("Keep all"));assert!(!output.contains("start_service_async"));}
    }
    app.edits.cycle(false);app.focus=Focus::Detail;
    assert!(!app.edits.overview);
    for (w,h) in [(80,24),(120,36),(40,14)] {
        let buffer=screen(&mut app,w,h,"edits-file");let output=text(&buffer);
        assert!(output.contains("bar.py"));
        if w>=80 {assert!(output.contains("+start_service_async"));assert!(buffer.content.iter().any(|c|c.fg==theme::GOOD));assert!(buffer.content.iter().any(|c|c.fg==theme::ERROR));}
    }
    app.viewport=(80,24);app.keep_edits(root.path(),&store,false);
    assert!(app.edits.files[0].kept);screen(&mut app,80,24,"edits-kept");
    app.prepare_revert(root.path(),&store,false);assert!(app.edits.pending.is_some());
    for (w,h) in [(80,24),(40,20)] {let output=text(&screen(&mut app,w,h,"edits-revert"));assert!(output.contains("proposed"));}
    let small=text(&screen(&mut app,40,14,"edits-revert-small"));
    assert!(small.contains("Revert disabled"));assert!(!small.contains("Revert [y]"));
    assert_eq!(std::fs::read_to_string(root.path().join("bar.py")).unwrap(),"start_service_async()\n");
    app.edits.pending=None;app.viewport=(80,24);std::fs::write(root.path().join("bar.py"),"manual adjustment\n").unwrap();
    app.keep_edits(root.path(),&store,false);
    let output=text(&screen(&mut app,80,24,"edits-drift"));assert!(output.contains("Needs attention"));
    assert!(app.diagnostic.as_ref().unwrap().contains("differs"));
    app.clear_diagnostic();app.edits.overview=true;app.keep_edits(root.path(),&store,true);
    assert_eq!(store.history(&session).unwrap().iter().filter(|e|e.2=="edits.kept").count(),1);
    assert_eq!(std::fs::read_to_string(root.path().join("bar.py")).unwrap(),"manual adjustment\n");
    assert_eq!(store.usage_report(&session,crate::usage::Scope::Session).unwrap().totals.requests,0);
}
