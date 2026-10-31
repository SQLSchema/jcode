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

    // Header from real App accessors: wordmark, session, short dir, branch.
    let header = &rows[0];
    assert!(header.contains("C R A C K E D"), "{header}");
    assert!(header.contains("sloth"), "session name missing: {header}");
    assert!(header.contains("jcode"), "short dir missing: {header}");
    assert!(header.contains("cracked-welcome"), "branch missing: {header}");
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
