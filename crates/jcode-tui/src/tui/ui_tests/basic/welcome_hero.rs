// Hero welcome screen (`[display] welcome_style = "hero"`).
//
// Renders through the real `ui::draw` entry point into a TestBackend and pins
// the contract: sprite present and centered, logo (or one-line fallback)
// present, composer below the hero, the hero disappears once a conversation
// exists, and `welcome_style = "classic"` restores the original screen.

/// RAII guard selecting a welcome style (and optionally the idle donut) via
/// env overrides for one test. Holds the shared env lock, acquired before the
/// render lock like the other env-mutating UI tests.
struct WelcomeStyleGuard {
    _env: std::sync::MutexGuard<'static, ()>,
}

impl WelcomeStyleGuard {
    fn set(style: &str, idle_animation: bool) -> Self {
        let env = crate::storage::lock_test_env();
        // The decorative-animation gate reads the perf tier, which is
        // first-initialization-wins; pin it so host load cannot flip it.
        crate::perf::pin_full_profile_for_tests();
        crate::env::set_var("JCODE_WELCOME_STYLE", style);
        if idle_animation {
            crate::env::set_var("JCODE_IDLE_ANIMATION", "1");
        } else {
            crate::env::remove_var("JCODE_IDLE_ANIMATION");
        }
        crate::config::invalidate_config_cache();
        Self { _env: env }
    }
}

impl Drop for WelcomeStyleGuard {
    fn drop(&mut self) {
        crate::env::remove_var("JCODE_WELCOME_STYLE");
        crate::env::remove_var("JCODE_IDLE_ANIMATION");
        crate::config::invalidate_config_cache();
    }
}

fn decorative_animation_allowed() -> bool {
    let policy = crate::perf::tui_policy();
    policy.enable_decorative_animations && policy.tier.idle_animation_enabled()
}

fn welcome_state() -> TestState {
    TestState {
        provider_model: Some("claude-opus-4-8".to_string()),
        provider_name: Some("anthropic".to_string()),
        working_dir: Some("C:\\Users\\dm101\\TUI\\jcode".to_string()),
        suppress_info_widgets: true,
        anim_elapsed: 0.0,
        ..Default::default()
    }
}

fn render_welcome(state: &TestState, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, state))
        .expect("welcome frame");
    terminal
}

fn welcome_text(terminal: &Terminal<TestBackend>) -> String {
    let buf = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut line = String::new();
        for x in 0..buf.area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn color_hex(color: ratatui::style::Color) -> Option<String> {
    use ratatui::style::Color;
    let (r, g, b) = match color {
        Color::Reset => return None,
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(n) => jcode_tui_style::indexed_to_rgb(n),
        Color::Black => (0, 0, 0),
        Color::White => (255, 255, 255),
        Color::Gray => (192, 192, 192),
        Color::DarkGray => (128, 128, 128),
        other => return Some(format!("{other:?}")),
    };
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// Cell dump (symbol + fg/bg) consumed by
/// `assets/custom/tools/render_preview.py` to rasterize a PNG preview.
fn welcome_cells_json(terminal: &Terminal<TestBackend>) -> serde_json::Value {
    let buf = terminal.backend().buffer();
    let rows: Vec<serde_json::Value> = (0..buf.area.height)
        .map(|y| {
            serde_json::Value::Array(
                (0..buf.area.width)
                    .map(|x| {
                        let cell = &buf[(x, y)];
                        serde_json::json!([
                            cell.symbol(),
                            color_hex(cell.fg),
                            color_hex(cell.bg),
                            cell.modifier.contains(ratatui::style::Modifier::BOLD),
                        ])
                    })
                    .collect(),
            )
        })
        .collect();
    serde_json::json!({
        "width": buf.area.width,
        "height": buf.area.height,
        "rows": rows,
    })
}

/// Write text/cell dumps when `JCODE_WELCOME_PREVIEW_DIR` is set, so previews
/// are regenerated from the exact frames the assertions inspected.
fn maybe_dump_welcome(terminal: &Terminal<TestBackend>, name: &str) {
    let Some(dir) = std::env::var_os("JCODE_WELCOME_PREVIEW_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("preview dir");
    std::fs::write(dir.join(format!("{name}.txt")), welcome_text(terminal)).expect("txt dump");
    std::fs::write(
        dir.join(format!("{name}.cells.json")),
        serde_json::to_string(&welcome_cells_json(terminal)).expect("json"),
    )
    .expect("cells dump");
}

fn sprite_cells_in(buf: &ratatui::buffer::Buffer, rect: Rect) -> usize {
    let mut n = 0;
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            let s = buf[(x, y)].symbol();
            if s == "▀" || s == "▄" {
                n += 1;
            }
        }
    }
    n
}

fn assert_hero_contract(width: u16, height: u16, expect_full_logo: bool) {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _lock = viewport_snapshot_test_lock();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let state = welcome_state();
    let terminal = render_welcome(&state, width, height);
    maybe_dump_welcome(&terminal, &format!("welcome_{width}x{height}"));
    let text = welcome_text(&terminal);
    let buf = terminal.backend().buffer();
    let hero = crate::tui::ui::welcome::take_last_hero_for_tests()
        .unwrap_or_else(|| panic!("{width}x{height} must render the hero:\n{text}"));

    // Sprite: present, horizontally centered, published as the animated rect.
    let sprite = hero.sprite.expect("sprite at this size");
    let left = sprite.x;
    let right = width - sprite.right();
    assert!(
        left.abs_diff(right) <= 1,
        "sprite off-center at {width}x{height}: {sprite:?}"
    );
    assert!(
        sprite_cells_in(buf, sprite) > (sprite.width as usize * sprite.height as usize) / 4,
        "sprite cells missing:\n{text}"
    );
    assert_eq!(
        crate::tui::ui::last_idle_animation_area(),
        Some(sprite),
        "the sprite rect must drive the animation-only repaint"
    );
    assert!(matches!(
        crate::tui::ui::last_idle_animation_kind(),
        crate::tui::ui::IdleAnimationKind::WelcomeSprite(_)
    ));

    // Logo: block art (full or half-block folded) or the one-line fallback.
    assert!(hero.logo.y > sprite.bottom().saturating_sub(1));
    if expect_full_logo {
        assert_ne!(
            hero.layout.logo,
            crate::tui::ui::welcome::LogoMode::Line,
            "{width}x{height} should fit the block logo"
        );
        let mut ink = 0;
        for y in hero.logo.top()..hero.logo.bottom() {
            for x in hero.logo.left()..hero.logo.right() {
                if !buf[(x, y)].symbol().trim().is_empty() {
                    ink += 1;
                }
            }
        }
        assert!(ink > 300, "logo missing ({ink} cells):\n{text}");
    } else {
        assert!(
            text.contains("C Y B E R P U N K") || text.contains("CYBERPUNK"),
            "fallback wordmark missing:\n{text}"
        );
    }

    // Composer: rounded box below the logo, centered, bounded, placeholder.
    assert!(hero.composer.y >= hero.logo.bottom(), "{hero:?}");
    assert!(hero.composer.width <= 80 && hero.composer.width <= width - 8);
    let c_left = hero.composer.x;
    let c_right = width - hero.composer.right();
    assert!(c_left.abs_diff(c_right) <= 1, "composer off-center: {hero:?}");
    assert_eq!(buf[(hero.composer.x, hero.composer.y)].symbol(), "╭");
    assert!(text.contains(crate::tui::ui::welcome::PLACEHOLDER), "{text}");
    assert!(text.contains("claude-opus-4-8"), "model row missing:\n{text}");
    assert!(text.contains("/resume") && text.contains("/help"), "{text}");
    // Footer: cwd left, version right; no classic chrome or donut.
    let last = text.lines().nth(height as usize - 1).unwrap_or("");
    assert!(last.contains("jcode") && last.contains("TUI"), "footer: {last:?}");
    assert!(!text.contains("/model to switch"), "classic header leaked:\n{text}");
    // Never draw past the right edge mid-glyph: every row fits.
    for row in text.lines() {
        assert!(unicode_width::UnicodeWidthStr::width(row) <= width as usize);
    }
}

#[test]
fn welcome_hero_renders_at_120x40() {
    assert_hero_contract(120, 40, true);
}

#[test]
fn welcome_hero_renders_at_80x24_with_fallback_wordmark() {
    assert_hero_contract(80, 24, false);
}

#[test]
fn welcome_hero_renders_at_reference_sizes() {
    for (w, h, full) in [(200, 50, true), (100, 30, true)] {
        assert_hero_contract(w, h, full);
    }
}

#[test]
fn welcome_hero_shows_for_logged_out_user_with_login_hint() {
    // A fresh install with no credentials lands on onboarding's resting
    // "Suggestions" state (a single `/login` suggestion). That is still an
    // empty screen, so the hero must own it and surface `/login` in its menu.
    let _style = WelcomeStyleGuard::set("hero", false);
    let _lock = viewport_snapshot_test_lock();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let mut state = welcome_state();
    state.onboarding_preview = true;
    state.suggestions = vec![("Log in to get started".to_string(), "/login".to_string())];
    let terminal = render_welcome(&state, 120, 40);
    let text = welcome_text(&terminal);
    assert!(
        crate::tui::ui::welcome::take_last_hero_for_tests().is_some(),
        "hero did not render for a logged-out user:\n{text}"
    );
    assert!(text.contains(crate::tui::ui::welcome::PLACEHOLDER), "{text}");
    assert!(text.contains("/login"), "missing /login hint:\n{text}");
    assert!(text.contains("Log in"), "missing login label:\n{text}");
    maybe_dump_welcome(&terminal, "welcome_logged_out_120x40");
}

#[test]
fn welcome_hero_menu_omits_login_when_authenticated() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _lock = viewport_snapshot_test_lock();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let state = welcome_state();
    let terminal = render_welcome(&state, 120, 40);
    let text = welcome_text(&terminal);
    assert!(crate::tui::ui::welcome::take_last_hero_for_tests().is_some());
    assert!(!text.contains("/login"), "unexpected /login for authed user:\n{text}");
    assert!(text.contains("/resume"), "{text}");
}

#[test]
fn welcome_hero_gives_way_once_a_conversation_exists() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(false);
    let _lock = viewport_snapshot_test_lock();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let mut state = welcome_state();
    state.display_messages = vec![DisplayMessage::user("hello".to_string())];
    let terminal = render_welcome(&state, 120, 40);
    let text = welcome_text(&terminal);
    assert!(crate::tui::ui::welcome::take_last_hero_for_tests().is_none());
    assert!(!text.contains("████"), "logo leaked into chat:\n{text}");
    assert!(!text.contains(crate::tui::ui::welcome::PLACEHOLDER));
    assert!(text.contains("hello"), "{text}");
    assert_eq!(crate::tui::ui::last_idle_animation_area(), None);
}

/// RAII guard for `JCODE_CORNER_SPRITE`. Callers already hold the env lock
/// through [`WelcomeStyleGuard`]. Enabling it also opts this test thread into
/// the cockpit layout (which supersedes the corner sprite).
struct CornerSpriteGuard {
    _cockpit: Option<crate::tui::ui::cockpit::CockpitTestOptIn>,
}

impl CornerSpriteGuard {
    fn set(enabled: bool) -> Self {
        crate::env::set_var("JCODE_CORNER_SPRITE", if enabled { "1" } else { "0" });
        crate::config::invalidate_config_cache();
        Self {
            _cockpit: enabled.then(crate::tui::ui::cockpit::CockpitTestOptIn::enable),
        }
    }
}

impl Drop for CornerSpriteGuard {
    fn drop(&mut self) {
        crate::env::remove_var("JCODE_CORNER_SPRITE");
        crate::config::invalidate_config_cache();
    }
}

fn conversation_state() -> TestState {
    let mut state = welcome_state();
    state.display_messages = vec![
        DisplayMessage::user("hello there".to_string()),
        DisplayMessage::assistant(
            "Hi! This is a long enough reply that it has to wrap across several rows so we can \
             check it never runs underneath the corner sprite in the top-right of the chat."
                .to_string(),
        ),
    ];
    state
}

#[test]
fn corner_sprite_stays_top_right_after_first_message() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let state = conversation_state();
    let terminal = render_welcome(&state, 120, 40);
    let text = welcome_text(&terminal);
    maybe_dump_welcome(&terminal, "corner_sprite_120x40");

    // The hero is gone but the sprite lives on in the corner.
    assert!(crate::tui::ui::welcome::take_last_hero_for_tests().is_none());
    assert!(text.contains("hello there"), "{text}");
    let area = crate::tui::ui::last_idle_animation_area().expect("corner sprite published");
    assert!(matches!(
        crate::tui::ui::last_idle_animation_kind(),
        crate::tui::ui::IdleAnimationKind::WelcomeSprite(_)
    ));
    // Top-right of the terminal: since the cockpit, the sprite sits at the top
    // of the right sidebar (under the one-row brand header).
    assert!(area.y <= 2, "{area:?}");
    assert!(area.right() >= 120 - 4 && area.right() <= 120, "{area:?}");
    let buf = terminal.backend().buffer();
    assert!(
        sprite_cells_in(buf, area) > (area.width as usize * area.height as usize) / 4,
        "corner sprite mostly empty:\n{text}"
    );
    // Transcript text never lands inside the sprite's gutter.
    for y in area.top()..area.bottom() {
        for x in area.left().saturating_sub(1)..area.right() {
            let s = buf[(x, y)].symbol();
            assert!(
                s == " " || s == "▀" || s == "▄" || s == "│",
                "transcript glyph {s:?} under corner sprite at ({x},{y}):\n{text}"
            );
        }
    }
}

#[test]
fn corner_sprite_hides_on_narrow_terminals_and_when_disabled() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _lock = viewport_snapshot_test_lock();
    {
        let _corner = CornerSpriteGuard::set(true);
        clear_flicker_frame_history_for_tests();
        crate::tui::ui::clear_test_render_state_for_tests();
        let terminal = render_welcome(&conversation_state(), 80, 24);
        let text = welcome_text(&terminal);
        assert_eq!(
            crate::tui::ui::last_idle_animation_area(),
            None,
            "80 columns is too tight for a corner sprite:\n{text}"
        );
    }
    {
        let _corner = CornerSpriteGuard::set(false);
        clear_flicker_frame_history_for_tests();
        crate::tui::ui::clear_test_render_state_for_tests();
        let _ = render_welcome(&conversation_state(), 120, 40);
        assert_eq!(crate::tui::ui::last_idle_animation_area(), None);
    }
}

#[test]
fn corner_sprite_keeps_its_cadence_while_the_model_is_thinking() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _corner = CornerSpriteGuard::set(true);
    let _lock = viewport_snapshot_test_lock();
    crate::perf::pin_full_profile_for_tests();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let mut state = conversation_state();
    state.status = ProcessingStatus::Thinking(std::time::Instant::now());
    let _ = render_welcome(&state, 120, 40);
    assert!(
        crate::tui::ui::last_idle_animation_area().is_some(),
        "corner sprite must stay on screen while a turn runs"
    );
    let interval = crate::tui::redraw_interval(&state);
    assert!(
        interval <= std::time::Duration::from_millis(100),
        "redraw slowed to {interval:?} while thinking, freezing the sprite"
    );
    assert!(crate::tui::periodic_redraw_required(&state));
}

#[test]
fn typing_a_draft_keeps_the_hero() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _lock = viewport_snapshot_test_lock();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let mut state = welcome_state();
    state.input = "hey rem".to_string();
    let terminal = render_welcome(&state, 120, 40);
    let text = welcome_text(&terminal);
    assert!(
        crate::tui::ui::welcome::take_last_hero_for_tests().is_some(),
        "hero vanished while typing:\n{text}"
    );
    assert!(text.contains("hey rem"), "{text}");
}

#[test]
fn welcome_style_classic_restores_the_original_screen_and_donut() {
    let _style = WelcomeStyleGuard::set("classic", true);
    let _lock = viewport_snapshot_test_lock();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let state = TestState {
        anim_elapsed: 1.0,
        ..Default::default()
    };
    let terminal = render_welcome(&state, 120, 40);
    let text = welcome_text(&terminal);
    assert!(crate::tui::ui::welcome::take_last_hero_for_tests().is_none());
    assert!(!text.contains(crate::tui::ui::welcome::PLACEHOLDER), "{text}");
    if !decorative_animation_allowed() {
        // Host perf policy disabled decorative animation (shared,
        // first-initialization-wins state in a parallel run): no donut to check.
        return;
    }
    let area = crate::tui::ui::last_idle_animation_area().expect("classic donut area");
    assert_eq!(
        crate::tui::ui::last_idle_animation_kind(),
        crate::tui::ui::IdleAnimationKind::Donut
    );
    assert!(area.height >= 2, "{area:?}");
}

#[test]
fn welcome_sprite_partial_repaint_matches_a_full_frame() {
    let _style = WelcomeStyleGuard::set("hero", false);
    let _lock = viewport_snapshot_test_lock();
    clear_flicker_frame_history_for_tests();
    crate::tui::ui::clear_test_render_state_for_tests();

    let mut state = welcome_state();
    state.anim_elapsed = 0.0;
    if !decorative_animation_allowed() {
        return;
    }
    assert!(
        crate::tui::idle_donut_active(&state),
        "a focused fresh hero screen animates the sprite"
    );
    let first = render_welcome(&state, 120, 40);
    let area = crate::tui::ui::last_idle_animation_area().expect("sprite rect");
    let mut patched = first.backend().buffer().clone();

    state.anim_elapsed = 0.25;
    let second = render_welcome(&state, 120, 40);
    let expected = second.backend().buffer();
    crate::tui::ui::render_idle_animation_into(&mut patched, area, 0.25);
    assert_ne!(
        first.backend().buffer().content,
        expected.content,
        "the sprite should advance between frames"
    );
    let mismatches = patched
        .content
        .iter()
        .zip(expected.content.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(mismatches, 0, "partial sprite repaint diverged from a full frame");
}
