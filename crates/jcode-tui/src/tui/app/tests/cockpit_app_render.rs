// Cockpit chat layout through the real `App` TuiState implementation (not the
// UI-test `TestState`): exercises App::git_branch, App::session_display_name,
// App::working_dir, token snapshot / agent rows from live App state, and the
// full `ui::draw` frame at the user's full-screen size.

#[test]
fn cockpit_renders_from_real_app_state_at_fullscreen() {
    let _env = crate::storage::lock_test_env();
    // Take the render lock BEFORE touching the env overrides and restore them
    // before releasing it: render tests that hold only the render lock (idle
    // hibernation, etc.) must never observe `JCODE_WELCOME_STYLE=hero`.
    let _render_lock = crate::tui::ui::render_state_test_lock();
    crate::perf::pin_full_profile_for_tests();
    crate::env::set_var("JCODE_WELCOME_STYLE", "hero");
    crate::env::set_var("JCODE_CORNER_SPRITE", "1");
    crate::config::invalidate_config_cache();
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::env::remove_var("JCODE_WELCOME_STYLE");
            crate::env::remove_var("JCODE_CORNER_SPRITE");
            crate::config::invalidate_config_cache();
            // The seeded branch is process-global; never leak it to later tests.
            crate::tui::app::helpers::seed_git_info_cache_for_tests(None);
        }
    }
    let _restore = Restore;
    let _cockpit = crate::tui::ui::cockpit::CockpitTestOptIn::enable();
    let _fixture =
        crate::tui::ui::header::scoped_unseen_changelog_entries_override_for_tests(Vec::new());
    crate::tui::app::helpers::seed_git_info_cache_for_tests(Some(
        crate::tui::info_widget::GitInfo {
            branch: "cracked-welcome".to_string(),
            ..Default::default()
        },
    ));

    let mut app = create_test_app();
    app.session.short_name = Some("sloth".to_string());
    app.session.working_dir = Some(r"C:\Users\dm101\TUI\jcode".to_string());
    app.push_display_message(DisplayMessage::user("hey rem, are you still there?".to_string()));
    app.push_display_message(DisplayMessage::assistant(
        "Still here! This reply sits in the main column, never under the sidebar.".to_string(),
    ));

    let (width, height) = (238u16, 64u16);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &app))
        .expect("cockpit frame");
    let buf = terminal.backend().buffer();
    let rows: Vec<String> = (0..height)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect();
    let text = rows.join("\n");

    // Header from real App accessors: 2-row block wordmark at full screen,
    // model + session on the first/second row, short dir + branch on the right.
    let big = crate::tui::ui::cockpit::WORDMARK_BIG;
    assert!(rows[0].contains(big[0]) && rows[1].contains(big[1]), "{}\n{}", rows[0], rows[1]);
    let header = format!("{}\n{}", rows[0], rows[1]);
    assert!(rows[1].contains("sloth"), "session name missing: {header}");
    assert!(rows[0].contains("jcode"), "short dir missing: {header}");
    assert!(rows[1].contains("cracked-welcome"), "branch missing: {header}");
    // No full path, no jcode header block, no changelog box.
    assert!(!text.contains("C:\\") && !text.contains("Users"), "{text}");
    assert!(!text.contains("Account router") && !text.contains("Updates"), "{text}");

    let rects = crate::tui::ui::cockpit::cockpit_layout(Rect::new(0, 0, width, height)).unwrap();
    let sidebar = rects.sidebar.expect("sidebar at full screen");
    // Sprite published inside the sidebar (drives the cheap partial repaint).
    let sprite = crate::tui::ui::last_idle_animation_area().expect("sprite published");
    assert!(sprite.x > sidebar.x && sprite.right() <= sidebar.right(), "{sprite:?}");
    assert_eq!((sprite.width, sprite.height), crate::tui::ui::welcome::sprite_cells(1));
    // Tokens card rendered from the real App token snapshot.
    let sidebar_text: String = (sidebar.top()..sidebar.bottom())
        .map(|y| (sidebar.x..sidebar.right()).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(sidebar_text.contains("tokens"), "tokens card missing:\n{sidebar_text}");
    assert!(!sidebar_text.contains("Still here"), "transcript in sidebar:\n{sidebar_text}");
    // Transcript in main, composer pinned right above the footer.
    assert!(text.contains("hey rem"), "{text}");
    let composer_bottom = rects.main.bottom() - 1;
    assert!(rows[composer_bottom as usize].contains('╰'), "{text}");
    let footer = &rows[height as usize - 1];
    assert!(footer.contains("/resume") && footer.contains("jcode v"), "{footer}");

    if let Some(dir) = std::env::var_os("JCODE_WELCOME_PREVIEW_DIR") {
        let dir = std::path::PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("cockpit_app_238x64.txt"), &text);
    }
}

#[test]
fn cockpit_agent_row_real_mouse_double_click_pops_out() {
    // Drive the real handle_mouse_event path (Down, Up, Down) on a published
    // hit row, the way a terminal reports a double-click. Live testing showed
    // the second press acting as a plain single click instead.
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let member = |id: &str, name: &str| crate::protocol::SwarmMemberStatus {
        session_id: id.into(),
        friendly_name: Some(name.into()),
        status: "running".into(),
        detail: None,
        task_label: None,
        role: Some("agent".into()),
        is_headless: Some(true),
        live_attachments: Some(0),
        status_age_secs: Some(1),
        output_tail: None,
        report_back_to_session_id: None,
        todo_progress: None,
        todo_items: Vec::new(),
        runtime: Default::default(),
    };
    let _lock = crate::tui::ui::render_state_test_lock();
    let mut app = App::cockpit_agents_test_app(vec![member("a", "fox"), member("b", "owl")]);
    crate::tui::ui::cockpit::record_agent_hit_rows(vec![(10, 100, 120, "b".into())]);
    let ev = |kind| MouseEvent { kind, column: 105, row: 10, modifiers: KeyModifiers::empty() };
    app.handle_mouse_event(ev(MouseEventKind::Down(MouseButton::Left)));
    app.handle_mouse_event(ev(MouseEventKind::Up(MouseButton::Left)));
    assert_eq!(app.expanded_agent.as_deref(), Some("b"), "single click expands");
    assert!(
        app.cockpit_agent_last_click.is_some(),
        "the first click must survive its own mouse-up so a second press can double-click"
    );
    app.handle_mouse_event(ev(MouseEventKind::Down(MouseButton::Left)));
    assert!(
        app.cockpit_agent_last_click.is_none(),
        "a second press inside the window is consumed as a double-click"
    );
    assert_eq!(app.expanded_agent.as_deref(), Some("b"), "double-click keeps the row open");
    crate::tui::ui::cockpit::record_agent_hit_rows(Vec::new());
}
