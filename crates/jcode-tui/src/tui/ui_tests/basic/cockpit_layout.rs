// Cockpit chat layout (fork): brand header, rounded composer, sidebar with the
// animated sprite + cards, footer. Rendered through the real `ui::draw` entry
// point. Reuses the welcome-hero helpers (guards, text/cell dumps).

fn cockpit_state() -> TestState {
    let mut state = conversation_state();
    state.display_messages.push(DisplayMessage::user(
        "and one more prompt that is long enough to wrap when the terminal is narrow, \
         so the transcript really fills the main column edge to edge, every row of it"
            .to_string(),
    ));
    state
}

fn render_cockpit(state: &TestState, width: u16, height: u16) -> Terminal<TestBackend> {
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();
    render_welcome(state, width, height)
}

fn row_text(buf: &ratatui::buffer::Buffer, y: u16, x0: u16, x1: u16) -> String {
    (x0..x1).map(|x| buf[(x, y)].symbol()).collect()
}

/// Find the rounded composer box inside `main`: returns (left, top, right, bottom).
fn composer_box(buf: &ratatui::buffer::Buffer, main: Rect) -> Option<(u16, u16, u16, u16)> {
    for y in (main.top()..main.bottom()).rev() {
        for x in main.left()..main.right() {
            if buf[(x, y)].symbol() == "╰" {
                let right = (x..main.right()).find(|&rx| buf[(rx, y)].symbol() == "╯")?;
                let top = (main.top()..y).rev().find(|&ty| buf[(x, ty)].symbol() == "╭")?;
                return Some((x, top, right, y));
            }
        }
    }
    None
}

fn assert_no_full_path(text: &str) {
    assert!(!text.contains("C:\\"), "full path leaked:\n{text}");
    assert!(!text.contains("Users"), "full path leaked:\n{text}");
}

fn assert_cockpit_contract(width: u16, height: u16) -> Terminal<TestBackend> {
    let state = cockpit_state();
    let terminal = render_cockpit(&state, width, height);
    maybe_dump_welcome(&terminal, &format!("cockpit_{width}x{height}"));
    let text = welcome_text(&terminal);
    let buf = terminal.backend().buffer();
    let rects = crate::tui::ui::cockpit::cockpit_layout(Rect::new(0, 0, width, height))
        .expect("cockpit at this size");

    // Header: wordmark + short dir, no full path anywhere.
    let header = text.lines().next().unwrap_or("");
    assert!(header.contains("CYBERPUNK") || header.contains("C Y B E R P U N K"), "{header:?}");
    assert!(header.contains("jcode"), "short dir missing: {header:?}");
    assert_no_full_path(&text);
    // jcode's persistent multi-line header is gone.
    assert!(!text.contains("/model to switch"), "classic header leaked:\n{text}");

    // Composer: rounded corners inside main.
    let (l, t, r, b) = composer_box(buf, rects.main)
        .unwrap_or_else(|| panic!("no rounded composer in main:\n{text}"));
    assert!(r < rects.main.right() && b < rects.main.bottom(), "composer escapes main");
    assert_eq!(buf[(l, t)].symbol(), "╭");
    assert_eq!(buf[(r, t)].symbol(), "╮");

    // Footer: menu + version; unknown context is omitted, never "ctx -".
    let footer = text.lines().nth(height as usize - 1).unwrap_or("");
    assert!(footer.contains("/resume") && footer.contains("/help"), "{footer:?}");
    assert!(footer.contains("jcode v") && !footer.contains("ctx -"), "{footer:?}");

    // Transcript is visible and starts right under the header.
    assert!(text.contains("hello there"), "{text}");

    if let Some(sidebar) = rects.sidebar {
        assert_eq!(sidebar.x, rects.main.right());
        for y in sidebar.top()..sidebar.bottom() {
            assert_eq!(buf[(sidebar.x, y)].symbol(), "│", "sidebar border at row {y}");
        }
        let sprite = crate::tui::ui::last_idle_animation_area().expect("sprite published");
        assert!(matches!(
            crate::tui::ui::last_idle_animation_kind(),
            crate::tui::ui::IdleAnimationKind::WelcomeSprite(_)
        ));
        assert!(
            sprite.x > sidebar.x && sprite.right() <= sidebar.right(),
            "sprite {sprite:?} outside sidebar {sidebar:?}"
        );
        assert!(sprite.y >= sidebar.y && sprite.bottom() <= sidebar.bottom());
        assert!(
            sprite_cells_in(buf, sprite) > (sprite.width as usize * sprite.height as usize) / 4,
            "sprite mostly empty:\n{text}"
        );
        // No transcript text inside the sidebar columns.
        for y in sidebar.top()..sidebar.bottom() {
            let row = row_text(buf, y, sidebar.x, sidebar.right());
            for needle in ["hello", "wrap", "corner", "prompt", "transcript"] {
                assert!(!row.contains(needle), "transcript in sidebar row {y}: {row:?}");
            }
        }
    } else {
        assert_eq!(crate::tui::ui::last_idle_animation_area(), None);
    }
    for row in text.lines() {
        assert!(unicode_width::UnicodeWidthStr::width(row) <= width as usize);
    }
    terminal
}

#[test]
fn cockpit_layout_140x40_has_sidebar() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    assert_cockpit_contract(140, 40);
}

#[test]
fn cockpit_layout_220x56_uses_wide_sidebar_and_big_sprite() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    assert_cockpit_contract(220, 56);
    let sprite = crate::tui::ui::last_idle_animation_area().expect("sprite");
    assert_eq!(
        (sprite.width, sprite.height),
        crate::tui::ui::welcome::sprite_cells(1),
        "48px sprite on a full-screen 1080p terminal"
    );
}

#[test]
fn cockpit_layout_238x64_fullscreen() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    let terminal = assert_cockpit_contract(238, 64);
    let text = welcome_text(&terminal);
    // No changelog box or jcode header block eating the top rows.
    assert!(!text.contains("Updates"), "{text}");
    assert!(!text.contains("Account router"), "{text}");
    // Composer pinned to the bottom of main, right above the footer, and the
    // short transcript bottom-anchored right above the composer (chat style).
    let rects = crate::tui::ui::cockpit::cockpit_layout(Rect::new(0, 0, 238, 64)).unwrap();
    let (_, top, _, bottom) = composer_box(terminal.backend().buffer(), rects.main).unwrap();
    assert_eq!(bottom, rects.main.bottom() - 1, "composer not pinned:\n{text}");
    assert_eq!(top, bottom - 2, "composer should be 3 rows:\n{text}");
    let last_text_row = (rects.main.top()..top)
        .rev()
        .find(|&y| !row_text(terminal.backend().buffer(), y, 0, rects.main.right() - 1).trim().is_empty())
        .expect("transcript rows");
    assert!(top - last_text_row <= 3, "transcript not bottom-anchored:\n{text}");
    assert!(!text.lines().nth(1).unwrap_or("").contains("hello there"), "{text}");
}

#[test]
fn cockpit_layout_100x30_has_no_sidebar() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    assert_cockpit_contract(100, 30);
}

#[test]
fn cockpit_layout_50x20_is_classic() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    let terminal = render_cockpit(&cockpit_state(), 50, 20);
    let text = welcome_text(&terminal);
    assert!(text.contains("hello there"), "{text}");
    assert!(!text.contains("C Y B E R P U N K"), "cockpit header at 50 cols:\n{text}");
    assert!(!text.contains("╰"), "rounded composer at 50 cols:\n{text}");
    assert_eq!(crate::tui::ui::last_idle_animation_area(), None);
}

#[test]
fn cockpit_off_when_corner_sprite_disabled_or_classic_style() {
    {
        let _style = WelcomeStyleGuard::set("hero", false);
        let _corner = CornerSpriteGuard::set(false);
        let _lock = viewport_snapshot_test_lock();
        let text = welcome_text(&render_cockpit(&cockpit_state(), 140, 40));
        assert!(!text.contains("C Y B E R P U N K"), "{text}");
        assert!(text.contains("/model to switch"), "classic header expected:\n{text}");
    }
    let _style = WelcomeStyleGuard::set("classic", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    let text = welcome_text(&render_cockpit(&cockpit_state(), 140, 40));
    assert!(!text.contains("C Y B E R P U N K"), "{text}");
    assert_eq!(crate::tui::ui::last_idle_animation_area(), None);
}

#[test]
fn cockpit_resize_across_sidebar_threshold_rewraps_transcript() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    let state = cockpit_state();
    // Wide first (narrower main), then narrow without the sidebar, then back.
    let _ = render_cockpit(&state, 140, 40);
    let narrow = render_cockpit(&state, 100, 30);
    let text = welcome_text(&narrow);
    // The transcript is re-wrapped for the 100-col main (a stale 114-col wrap
    // would clip the end of the long prompt).
    let rects = crate::tui::ui::cockpit::cockpit_layout(Rect::new(0, 0, 100, 30)).unwrap();
    assert!(rects.sidebar.is_none());
    assert!(text.contains("hello there"), "{text}");
    assert!(text.contains("row of it"), "prompt tail clipped:\n{text}");
    let wide = render_cockpit(&state, 140, 40);
    let wide_text = welcome_text(&wide);
    let wide_buf = wide.backend().buffer();
    let sidebar = crate::tui::ui::cockpit::cockpit_layout(Rect::new(0, 0, 140, 40))
        .unwrap()
        .sidebar
        .unwrap();
    for y in sidebar.top()..sidebar.bottom() {
        let row = row_text(wide_buf, y, sidebar.x, sidebar.right());
        for needle in ["hello", "wrap", "corner", "prompt", "transcript"] {
            assert!(!row.contains(needle), "stale wrap in sidebar row {y}: {row:?}\n{wide_text}");
        }
    }
    assert!(wide_text.contains("row of it"), "prompt tail clipped:\n{wide_text}");
    assert_no_full_path(&text);
    assert_no_full_path(&wide_text);
}
