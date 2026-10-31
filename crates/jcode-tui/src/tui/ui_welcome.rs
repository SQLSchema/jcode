//! OpenCode-style "hero" welcome screen for a fresh session.
//!
//! Shown while `[display] welcome_style = "hero"` (the default) and the session
//! has no conversation yet. Top to bottom, vertically centered in the chat
//! column: an animated pixel-art sprite, the CYBERPUNK logo, a
//! bounded rounded composer, a dim model row and a slash-command menu row. A
//! footer carries the working directory (left) and version (right).
//!
//! The sprite is the only moving element. It is published through the shared
//! idle-animation slot (`record_idle_animation`) so the run loop's
//! animation-only partial repaint redraws just the sprite rectangle, exactly
//! like the classic idle donut. Once a real message exists the normal chat
//! layout takes over unchanged.

use super::animations::{IdleAnimationKind, record_idle_animation};
use super::visual_debug::FrameCaptureBuilder;
use super::{TuiState, accent_color, dim_color, input_ui};
use crate::tui::color_support::rgb;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders},
};
use std::sync::OnceLock;
use unicode_width::UnicodeWidthStr;

/// Sprite sheet frame rate (matches `welcome_assets/sprite.json`).
pub(crate) const SPRITE_FPS: f32 = 12.0;
/// Frames per sheet (matches `welcome_assets/sprite.json`).
const SPRITE_FRAMES: usize = 7;
/// Frame edge length in pixels for each embedded sheet, biggest first.
pub(crate) const SPRITE_SIZES: [u16; 4] = [64, 48, 36, 24];
const SPRITE_PNGS: [&[u8]; 4] = [
    include_bytes!("welcome_assets/sprite_64.png"),
    include_bytes!("welcome_assets/sprite_48.png"),
    include_bytes!("welcome_assets/sprite_36.png"),
    include_bytes!("welcome_assets/sprite_24.png"),
];
const LOGO_TXT: &str = include_str!("welcome_assets/logo.txt");
const LOGO_FALLBACK: &str = "CYBERPUNK";
pub(crate) const PLACEHOLDER: &str = "Say something, or type / for commands...";

/// Deep blue start of the brand gradient (#3d5afe).
const GRAD_FROM: (u8, u8, u8) = (0x3d, 0x5a, 0xfe);
/// Lavender end of the brand gradient (#b4c0ff).
const GRAD_TO: (u8, u8, u8) = (0xb4, 0xc0, 0xff);
/// Brightness of `▒` shadow cells relative to solid `█` cells.
const SHADE_FACTOR: f32 = 0.5;

/// Whether the hero welcome screen is wanted for this state: the configured
/// style is hero, onboarding is not driving the screen, and no conversation
/// has started yet. Layout may still fall back to classic when the terminal
/// is too small (see [`choose_layout`]).
pub(crate) fn welcome_hero_wanted(state: &dyn TuiState) -> bool {
    configured_welcome_style() == jcode_config_types::WelcomeStyle::Hero
        && !guided_onboarding_active(state)
        && !conversation_started(state)
        && !state.is_processing()
        && state.streaming_text().is_empty()
        && state.queued_messages().is_empty()
}

/// Whether the guided onboarding flow (login import, OpenAI prompt, continue
/// prompt) owns the screen. Those phases are key-driven and must win. The
/// resting `Suggestions` state is *not* guided: it is just the empty screen of
/// a user who has not logged in yet, and the hero shows there with a `/login`
/// hint instead of the plain classic hint.
fn guided_onboarding_active(state: &dyn TuiState) -> bool {
    state.onboarding_welcome_active()
        && state.onboarding_welcome_kind() != crate::tui::OnboardingWelcomeKind::Suggestions
}

/// The empty screen of a user with no working credentials asks them to log in.
fn needs_login(state: &dyn TuiState) -> bool {
    state
        .suggestion_prompts()
        .iter()
        .any(|(_, prompt)| prompt.trim() == "/login")
}

fn configured_welcome_style() -> jcode_config_types::WelcomeStyle {
    // The unit-test suite pins hundreds of classic empty-screen layouts. Keep
    // them meaningful by making the hero opt-in under `cfg(test)`: tests that
    // exercise it set `JCODE_WELCOME_STYLE` explicitly.
    #[cfg(test)]
    if std::env::var_os("JCODE_WELCOME_STYLE").is_none() {
        return jcode_config_types::WelcomeStyle::Classic;
    }
    crate::config::config().display.welcome_style
}

fn conversation_started(state: &dyn TuiState) -> bool {
    state.display_messages().iter().any(|m| {
        matches!(
            m.role.as_str(),
            "user" | "assistant" | "tool" | "reasoning" | "error"
        )
    })
}

// ---------------------------------------------------------------- sprite ---

struct SpriteSheet {
    size: usize,
    sheet_width: usize,
    /// Row-major RGBA pixels of the whole horizontal sheet.
    pixels: Vec<[u8; 4]>,
}

impl SpriteSheet {
    fn pixel(&self, frame: usize, x: usize, y: usize) -> Option<(u8, u8, u8)> {
        let sx = frame * self.size + x;
        if x >= self.size || y >= self.size || sx >= self.sheet_width {
            return None;
        }
        let [r, g, b, a] = *self.pixels.get(y * self.sheet_width + sx)?;
        (a >= 128).then_some((r, g, b))
    }
}

fn sprite_sheets() -> &'static [Option<SpriteSheet>] {
    static SHEETS: OnceLock<Vec<Option<SpriteSheet>>> = OnceLock::new();
    SHEETS.get_or_init(|| {
        SPRITE_PNGS
            .iter()
            .zip(SPRITE_SIZES)
            .map(|(bytes, size)| {
                let img = image::load_from_memory(bytes).ok()?.to_rgba8();
                let (w, h) = img.dimensions();
                if h < u32::from(size) {
                    return None;
                }
                Some(SpriteSheet {
                    size: size as usize,
                    sheet_width: w as usize,
                    pixels: img.pixels().map(|p| p.0).collect(),
                })
            })
            .collect()
    })
}

/// Sprite frame shown at `elapsed` seconds of animation time.
pub(crate) fn sprite_frame_index(elapsed: f32) -> usize {
    ((elapsed.max(0.0) * SPRITE_FPS) as usize) % SPRITE_FRAMES
}

/// Cell footprint of a sprite size: one column per pixel, two pixel rows per
/// cell row (upper-half-block rendering).
pub(crate) fn sprite_cells(size_idx: usize) -> (u16, u16) {
    let size = SPRITE_SIZES[size_idx.min(SPRITE_SIZES.len() - 1)];
    (size, size.div_ceil(2))
}

/// Paint one sprite frame into `area` with half-block pixels. Transparent
/// pixels leave the terminal default (`Color::Reset`) showing through.
pub(crate) fn render_sprite_frame(buf: &mut Buffer, area: Rect, size_idx: usize, frame: usize) {
    let Some(Some(sheet)) = sprite_sheets().get(size_idx) else {
        return;
    };
    let area = area.intersection(*buf.area());
    let frame = frame % SPRITE_FRAMES;
    for cy in 0..area.height as usize {
        for cx in 0..area.width as usize {
            let top = sheet.pixel(frame, cx, cy * 2);
            let bottom = sheet.pixel(frame, cx, cy * 2 + 1);
            let cell = &mut buf[(area.x + cx as u16, area.y + cy as u16)];
            cell.reset();
            match (top, bottom) {
                (Some(t), Some(b)) => {
                    cell.set_char('▀')
                        .set_fg(rgb(t.0, t.1, t.2))
                        .set_bg(rgb(b.0, b.1, b.2));
                }
                (Some(t), None) => {
                    cell.set_char('▀').set_fg(rgb(t.0, t.1, t.2));
                }
                (None, Some(b)) => {
                    cell.set_char('▄').set_fg(rgb(b.0, b.1, b.2));
                }
                (None, None) => {
                    cell.set_char(' ');
                }
            }
        }
    }
}

// ------------------------------------------------------------------ logo ---

struct Logo {
    rows: Vec<Vec<char>>,
    width: usize,
    /// ASCII glyph art (no block characters): every non-space char is ink,
    /// drawn as-is, and the half-block fold is skipped.
    glyph: bool,
}

fn logo() -> &'static Logo {
    static LOGO: OnceLock<Logo> = OnceLock::new();
    LOGO.get_or_init(|| {
        let mut rows: Vec<Vec<char>> = LOGO_TXT
            .lines()
            .map(|line| line.trim_end().chars().collect())
            .collect();
        while rows.last().is_some_and(|row| row.is_empty()) {
            rows.pop();
        }
        while rows.first().is_some_and(|row| row.is_empty()) {
            rows.remove(0);
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        let glyph = !rows
            .iter()
            .flatten()
            .any(|ch| matches!(ch, '█' | '▒' | '░' | '▓'));
        Logo { rows, width, glyph }
    })
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)).round() as u8
}

fn gradient(t: f32, scale: f32) -> (u8, u8, u8) {
    let r = lerp(GRAD_FROM.0, GRAD_TO.0, t) as f32 * scale;
    let g = lerp(GRAD_FROM.1, GRAD_TO.1, t) as f32 * scale;
    let b = lerp(GRAD_FROM.2, GRAD_TO.2, t) as f32 * scale;
    (r as u8, g as u8, b as u8)
}

/// Diagonal brand gradient: mostly left-to-right, a little top-to-bottom.
fn logo_color(col: usize, row: usize, width: usize, rows: usize, shade: bool) -> Color {
    let tx = col as f32 / width.saturating_sub(1).max(1) as f32;
    let ty = row as f32 / rows.saturating_sub(1).max(1) as f32;
    let (r, g, b) = gradient(tx * 0.75 + ty * 0.25, if shade { SHADE_FACTOR } else { 1.0 });
    rgb(r, g, b)
}

fn logo_pixel(logo: &Logo, col: usize, row: usize) -> Option<Color> {
    let ch = *logo.rows.get(row)?.get(col)?;
    match ch {
        '█' => Some(logo_color(col, row, logo.width, logo.rows.len(), false)),
        '▒' | '░' | '▓' => Some(logo_color(col, row, logo.width, logo.rows.len(), true)),
        // ASCII glyph art (figlet-style fonts): every visible character is ink.
        c if logo.glyph && !c.is_whitespace() => {
            Some(logo_color(col, row, logo.width, logo.rows.len(), false))
        }
        _ => None,
    }
}

/// How the logo is drawn, from most to least vertical space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LogoMode {
    /// The block art as-is (`█` solid, `▒` dimmer shadow).
    Full,
    /// Two art rows folded into one with half blocks.
    Half,
    /// Single-line bold gradient wordmark.
    Line,
}

impl LogoMode {
    fn rows(self) -> u16 {
        let logo = logo();
        match self {
            Self::Full => logo.rows.len() as u16,
            Self::Half => logo.rows.len().div_ceil(2) as u16,
            Self::Line => 1,
        }
    }

    fn width(self) -> u16 {
        match self {
            Self::Full | Self::Half => logo().width as u16,
            Self::Line => LOGO_FALLBACK.chars().count() as u16 * 2 - 1,
        }
    }
}

fn draw_logo(buf: &mut Buffer, area: Rect, mode: LogoMode) {
    let logo = logo();
    let width = mode.width().min(area.width);
    let x0 = area.x + (area.width - width) / 2;
    match mode {
        LogoMode::Full => {
            for (row, chars) in logo.rows.iter().enumerate() {
                let y = area.y + row as u16;
                if y >= area.bottom() {
                    break;
                }
                for (col, &ch) in chars.iter().enumerate() {
                    if col as u16 >= width {
                        break;
                    }
                    if let Some(color) = logo_pixel(logo, col, row) {
                        buf[(x0 + col as u16, y)].set_char(ch).set_fg(color);
                    }
                }
            }
        }
        LogoMode::Half => {
            for cy in 0..mode.rows() {
                let y = area.y + cy;
                if y >= area.bottom() {
                    break;
                }
                for col in 0..width as usize {
                    let top = logo_pixel(logo, col, cy as usize * 2);
                    let bottom = logo_pixel(logo, col, cy as usize * 2 + 1);
                    let cell = &mut buf[(x0 + col as u16, y)];
                    match (top, bottom) {
                        (Some(t), Some(b)) => {
                            cell.set_char('▀').set_fg(t).set_bg(b);
                        }
                        (Some(t), None) => {
                            cell.set_char('▀').set_fg(t);
                        }
                        (None, Some(b)) => {
                            cell.set_char('▄').set_fg(b);
                        }
                        (None, None) => {}
                    }
                }
            }
        }
        LogoMode::Line => {
            let spaced = width >= mode.width();
            let chars: Vec<char> = LOGO_FALLBACK.chars().collect();
            let n = chars.len();
            let total = if spaced { n * 2 - 1 } else { n };
            let x0 = area.x + (area.width.saturating_sub(total as u16)) / 2;
            for (i, ch) in chars.into_iter().enumerate() {
                let x = x0 + if spaced { i as u16 * 2 } else { i as u16 };
                if x >= area.right() {
                    break;
                }
                let (r, g, b) = gradient(i as f32 / (n - 1) as f32, 1.0);
                buf[(x, area.y)]
                    .set_char(ch)
                    .set_fg(rgb(r, g, b))
                    .set_style(Style::default().add_modifier(Modifier::BOLD));
            }
        }
    }
}

// ---------------------------------------------------------------- layout ---

/// Hero arrangement chosen for a terminal size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeroLayout {
    /// Index into [`SPRITE_SIZES`], or `None` when the sprite is dropped.
    pub sprite: Option<usize>,
    pub logo: LogoMode,
    /// Blank row between the model row and the menu row.
    pub menu_gap: bool,
}

impl HeroLayout {
    fn height(self, composer_rows: u16) -> u16 {
        let sprite = self.sprite.map(|idx| sprite_cells(idx).1 + 1).unwrap_or(0);
        sprite + self.logo.rows() + 1 + composer_rows + 1 + u16::from(self.menu_gap) + 1
    }
}

/// Pick the richest hero that fits `width` x `height` (the area above the
/// footer), or `None` when even the minimal hero does not fit and the classic
/// layout should be used instead. Larger sprites win, but the full block
/// logo is preferred over a much smaller sprite.
pub(crate) fn choose_layout(width: u16, height: u16, composer_rows: u16) -> Option<HeroLayout> {
    use LogoMode::*;
    const ORDER: &[(Option<usize>, LogoMode)] = &[
        (Some(0), Full),
        (Some(1), Full),
        (Some(0), Half),
        (Some(1), Half),
        (Some(2), Full),
        (Some(2), Half),
        (Some(3), Full),
        (Some(3), Half),
        (Some(2), Line),
        (Some(3), Line),
        (None, Full),
        (None, Half),
        (None, Line),
    ];
    // Keep a row of breathing room so the group never touches the edges.
    let budget = height.saturating_sub(1);
    for &(sprite, logo) in ORDER {
        for menu_gap in [true, false] {
            if logo != Line && width < logo.width() + 4 {
                continue;
            }
            // Folding two rows into half blocks only makes sense for block art.
            if logo == Half && self::logo().glyph {
                continue;
            }
            if logo == Line && width < LOGO_FALLBACK.len() as u16 + 2 {
                continue;
            }
            if let Some(idx) = sprite
                && (sprite_cells(idx).0 + 4 > width
                    || sprite_sheets().get(idx).is_none_or(Option::is_none))
            {
                continue;
            }
            let layout = HeroLayout {
                sprite,
                logo,
                menu_gap,
            };
            if layout.height(composer_rows) <= budget {
                return Some(layout);
            }
        }
    }
    None
}

/// Composer box width on the hero: bounded and centered under the logo.
pub(crate) fn composer_box_width(area_width: u16) -> u16 {
    if area_width < 28 {
        area_width
    } else {
        area_width.saturating_sub(8).min(80)
    }
}

// ------------------------------------------------------------------ draw ---

/// Rectangles the hero laid out, for the caller's bookkeeping and tests.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct HeroRects {
    pub layout: HeroLayout,
    pub sprite: Option<Rect>,
    pub logo: Rect,
    pub composer: Rect,
    pub input: Rect,
    pub footer: Rect,
}

fn border_color() -> Color {
    rgb(122, 134, 230)
}

fn model_color() -> Color {
    rgb(180, 192, 255)
}

fn compact_version(version: &str) -> String {
    let trimmed = version.trim();
    match trimmed.split_once(" (") {
        Some((head, _)) => head.trim().to_string(),
        None => trimmed.to_string(),
    }
}

fn display_cwd(app: &dyn TuiState) -> String {
    let raw = app
        .working_dir()
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        let home = home.display().to_string();
        if !home.is_empty() && raw.starts_with(&home) {
            return format!("~{}", &raw[home.len()..]);
        }
    }
    raw
}

/// Truncate from the left with a leading ellipsis so the tail stays visible.
fn truncate_left(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::new();
    let mut used = 1;
    for &ch in chars.iter().rev() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > max {
            break;
        }
        used += w;
        out.push(ch);
    }
    out.reverse();
    format!("…{}", out.into_iter().collect::<String>())
}

fn truncate_right(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        used += w;
        out.push(ch);
    }
    out.push('…');
    out
}

fn put_line(buf: &mut Buffer, x: u16, y: u16, max_x: u16, line: &Line<'_>) {
    let mut x = x;
    for span in &line.spans {
        if x >= max_x {
            break;
        }
        let (nx, _) = buf.set_stringn(x, y, span.content.as_ref(), (max_x - x) as usize, span.style);
        x = nx;
    }
}

fn menu_line(width: u16, needs_login: bool) -> Line<'static> {
    let items: &[(&str, &str)] = if needs_login {
        &[
            ("/login", "Log in"),
            ("/resume", "Sessions"),
            ("/config", "Settings"),
            ("/help", "Help"),
        ]
    } else {
        &[
            ("/resume", "Sessions"),
            ("/config", "Settings"),
            ("/help", "Help"),
        ]
    };
    let full: usize = items
        .iter()
        .map(|(cmd, label)| cmd.len() + 1 + label.len())
        .sum::<usize>()
        + 3 * (items.len() - 1);
    let with_labels = full <= width as usize;
    let mut spans = Vec::new();
    for (i, (cmd, label)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(if with_labels { "   " } else { "  " }));
        }
        spans.push(Span::styled(*cmd, Style::default().fg(accent_color())));
        if with_labels {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::default().fg(dim_color()),
            ));
        }
    }
    Line::from(spans)
}

/// Draw the hero into `area`. Returns `None` (having drawn nothing) when the
/// area is too small, so the caller can fall back to the classic layout.
pub(super) fn draw_hero(
    frame: &mut Frame,
    app: &dyn TuiState,
    area: Rect,
    next_prompt: usize,
    debug_capture: &mut Option<FrameCaptureBuilder>,
) -> Option<HeroRects> {
    if area.width < 24 || area.height < 10 {
        return None;
    }
    let box_width = composer_box_width(area.width);
    let inner_width = box_width.saturating_sub(2);
    let input_rows = input_ui::wrapped_input_line_count(app, inner_width, next_prompt).min(6) as u16
        + input_ui::input_hint_line_height(app);
    let composer_rows = input_rows + 2;

    // Bottom rows: footer (cwd / version) and an optional notice line for the
    // latest system message (e.g. "run /login when you're ready").
    let notice = app
        .display_messages()
        .iter()
        .rev()
        .find(|m| m.role == "system" && !m.content.trim().is_empty())
        .and_then(|m| m.content.lines().find(|l| !l.trim().is_empty()))
        .map(|l| l.trim().to_string());
    let reserved_bottom = 1 + u16::from(notice.is_some());
    let hero_height = area.height.saturating_sub(reserved_bottom);
    let layout = choose_layout(area.width, hero_height, composer_rows)?;

    let total = layout.height(composer_rows);
    // Sit slightly above true center, like the reference look.
    let top = area.y + hero_height.saturating_sub(total) * 2 / 5;
    let mut y = top;

    let mut sprite_rect = None;
    if let Some(idx) = layout.sprite {
        let (w, h) = sprite_cells(idx);
        let rect = Rect::new(area.x + (area.width - w) / 2, y, w, h);
        // Animate only while the shared idle-animation gate allows it (focused,
        // not deep idle, perf tier); otherwise hold a stable first frame.
        let frame_idx = if crate::tui::sprite_may_animate(app) {
            sprite_frame_index(app.animation_elapsed())
        } else {
            0
        };
        render_sprite_frame(frame.buffer_mut(), rect, idx, frame_idx);
        record_idle_animation(Some(rect), IdleAnimationKind::WelcomeSprite(idx as u8));
        sprite_rect = Some(rect);
        y += h + 1;
    }

    let logo_rect = Rect::new(area.x, y, area.width, layout.logo.rows());
    draw_logo(frame.buffer_mut(), logo_rect, layout.logo);
    y += layout.logo.rows() + 1;

    let composer = Rect::new(
        area.x + (area.width - box_width) / 2,
        y,
        box_width,
        composer_rows,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color()));
    let input = block.inner(composer);
    frame.render_widget(block, composer);
    let cursor = input_ui::draw_input(frame, app, input, next_prompt, debug_capture);
    if app.input().is_empty()
        && app.login_input_hint().is_none()
        && let Some(cursor) = cursor
        && cursor.x < input.right()
    {
        let reserved = input_ui::send_mode_reserved_width(app) as u16;
        let max = input.right().saturating_sub(reserved).saturating_sub(cursor.x);
        frame.buffer_mut().set_stringn(
            cursor.x,
            cursor.y,
            PLACEHOLDER,
            max as usize,
            Style::default().fg(dim_color()),
        );
    }
    y = composer.bottom();

    // Model row, aligned with the composer's text column.
    let model = app.provider_model();
    let provider = app.provider_name();
    let model_x = composer.x + 2;
    let max_x = composer.right();
    let mut spans = vec![Span::styled(
        model.clone(),
        Style::default()
            .fg(model_color())
            .add_modifier(Modifier::BOLD),
    )];
    if !provider.trim().is_empty() && !model.contains(&provider) {
        spans.push(Span::styled(" · ", Style::default().fg(dim_color())));
        spans.push(Span::styled(provider, Style::default().fg(dim_color())));
    }
    put_line(frame.buffer_mut(), model_x, y, max_x, &Line::from(spans));
    y += 1 + u16::from(layout.menu_gap);

    let menu = menu_line(area.width, needs_login(app));
    let menu_width = menu.width() as u16;
    put_line(
        frame.buffer_mut(),
        area.x + area.width.saturating_sub(menu_width) / 2,
        y,
        area.right(),
        &menu,
    );

    // Footer: cwd bottom-left, version bottom-right.
    let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let version = format!("jcode {}", compact_version(jcode_build_meta::version()));
    let version = truncate_right(&version, (area.width as usize).saturating_sub(4) / 2);
    let version_width = version.width() as u16;
    let cwd_max = (area.width as usize).saturating_sub(version_width as usize + 4);
    let cwd = truncate_left(&display_cwd(app), cwd_max);
    let dim = Style::default().fg(dim_color());
    frame
        .buffer_mut()
        .set_stringn(area.x + 1, footer.y, &cwd, cwd_max, dim);
    frame.buffer_mut().set_stringn(
        area.right().saturating_sub(version_width + 1),
        footer.y,
        &version,
        version_width as usize,
        dim,
    );
    if let Some(notice) = notice {
        let notice = truncate_right(&notice, (area.width as usize).saturating_sub(4));
        let w = notice.width() as u16;
        frame.buffer_mut().set_stringn(
            area.x + (area.width - w) / 2,
            footer.y - 1,
            &notice,
            w as usize,
            dim,
        );
    }

    let rects = HeroRects {
        layout,
        sprite: sprite_rect,
        logo: logo_rect,
        composer,
        input,
        footer,
    };
    #[cfg(test)]
    LAST_HERO.with(|slot| slot.set(Some(rects)));
    Some(rects)
}

// ---------------------------------------------------------- corner sprite ---

/// Where the corner sprite goes once a conversation is running: which sheet
/// size, and how many columns the transcript gives up on the right for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CornerSprite {
    pub size_idx: usize,
    pub gutter: u16,
}

/// Transcript columns that must stay usable next to the corner sprite.
const CORNER_MIN_TRANSCRIPT: u16 = 70;

/// Whether the corner sprite is wanted: the hero style is active, the user
/// has not turned it off, and the hero has handed over to a real conversation.
pub(crate) fn corner_sprite_wanted(state: &dyn TuiState) -> bool {
    configured_welcome_style() == jcode_config_types::WelcomeStyle::Hero
        && configured_corner_sprite()
        && conversation_started(state)
}

fn configured_corner_sprite() -> bool {
    crate::config::config().display.corner_sprite
}

/// Pick a corner sprite that fits `chat_area`, or `None` when the terminal is
/// too small to spare a gutter without cramping the transcript.
pub(crate) fn corner_sprite_layout(state: &dyn TuiState, chat_area: Rect) -> Option<CornerSprite> {
    if !corner_sprite_wanted(state) {
        return None;
    }
    // 36px on roomy terminals, 24px otherwise (indices into SPRITE_SIZES).
    let candidates: &[(usize, u16, u16)] = &[(2, 160, 40), (3, 0, 0)];
    for &(size_idx, min_width, min_height) in candidates {
        let (w, h) = sprite_cells(size_idx);
        let gutter = w + 2;
        if chat_area.width >= min_width.max(gutter + CORNER_MIN_TRANSCRIPT)
            && chat_area.height >= min_height.max(h + 10)
        {
            return Some(CornerSprite { size_idx, gutter });
        }
    }
    None
}

/// Draw the corner sprite and publish it to the idle-animation slot so the
/// run loop can animate it with the cheap sprite-only repaint.
pub(crate) fn draw_corner_sprite(frame: &mut Frame, app: &dyn TuiState, rect: Rect, size_idx: usize) {
    let frame_idx = if crate::tui::sprite_may_animate(app) {
        sprite_frame_index(app.animation_elapsed())
    } else {
        0
    };
    render_sprite_frame(frame.buffer_mut(), rect, size_idx, frame_idx);
    record_idle_animation(Some(rect), IdleAnimationKind::WelcomeSprite(size_idx as u8));
}

#[cfg(test)]
thread_local! {
    static LAST_HERO: std::cell::Cell<Option<HeroRects>> = const { std::cell::Cell::new(None) };
}

/// Rectangles of the most recent hero drawn on this thread (tests only).
#[cfg(test)]
pub(crate) fn take_last_hero_for_tests() -> Option<HeroRects> {
    LAST_HERO.with(|slot| slot.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_decode() {
        for (idx, sheet) in sprite_sheets().iter().enumerate() {
            let sheet = sheet.as_ref().expect("sprite sheet decodes");
            assert_eq!(sheet.size, SPRITE_SIZES[idx] as usize);
            assert!(sheet.sheet_width >= sheet.size * SPRITE_FRAMES);
        }
        let logo = logo();
        assert_eq!(logo.rows.len(), 6);
        assert!(logo.width >= 90 && logo.width <= 96, "{}", logo.width);
        assert!(logo.glyph, "CYBERPUNK art is ASCII glyph art");
        // Every visible character is ink in glyph mode.
        assert!(logo_pixel(logo, 2, 0).is_some(), "{:?}", logo.rows[0]);
        assert!(logo_pixel(logo, 0, 0).is_none());
    }

    #[test]
    fn glyph_logo_never_uses_the_half_block_fold() {
        for (w, h) in [(80, 24), (120, 30), (140, 40), (238, 64), (100, 20)] {
            if let Some(layout) = choose_layout(w, h, 3) {
                assert_ne!(layout.logo, LogoMode::Half, "{w}x{h}: {layout:?}");
            }
        }
    }

    #[test]
    fn layout_is_responsive_at_reference_sizes() {
        let pick = |w: u16, h: u16| {
            choose_layout(w, h - 1, 3).unwrap_or_else(|| panic!("{w}x{h} has a hero"))
        };
        let big = pick(200, 50);
        assert!(big.sprite.is_some() && big.logo != LogoMode::Line, "{big:?}");
        let mid = pick(120, 40);
        assert!(mid.sprite.is_some() && mid.logo != LogoMode::Line, "{mid:?}");
        let small = pick(100, 30);
        assert_ne!(small.logo, LogoMode::Line, "{small:?}");
        let narrow = pick(80, 24);
        assert_eq!(narrow.logo, LogoMode::Line, "{narrow:?}");
        assert!(narrow.sprite.is_some(), "{narrow:?}");
        // Very short terminals drop the sprite before giving up.
        let short = choose_layout(120, 12, 3).expect("short hero");
        assert_eq!(short.sprite, None);
        assert_eq!(choose_layout(120, 6, 3), None);
    }

    #[test]
    fn frame_index_cycles_at_sprite_fps() {
        assert_eq!(sprite_frame_index(0.0), 0);
        assert_eq!(sprite_frame_index(1.0 / SPRITE_FPS + 0.001), 1);
        assert_eq!(sprite_frame_index(SPRITE_FRAMES as f32 / SPRITE_FPS + 0.001), 0);
    }
}
