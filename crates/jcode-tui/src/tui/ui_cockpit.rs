//! "Cockpit" chat layout for the cracked fork.
//!
//! Once a conversation starts, the chat column gets a one-row brand header,
//! a right sidebar (animated sprite, tokens card, agents card), a rounded
//! composer and a footer. Shapes match the welcome hero so the two screens
//! read as one app.
//!
//! Ownership (parallel lanes, keep to your section):
//! - layout + header + composer + footer + wiring in ui.rs: lane "cockpit-layout"
//! - tokens card + `TokenSnapshot` population:            lane "tokens-card"
//! - agents card + click / double-click handling:         lane "agents-card"
//!
//! Shared contract (frozen by the coordinator, change only with agreement):
//! - [`CockpitRects`] and [`cockpit_layout`] decide every region.
//! - Sidebar cards are drawn by [`draw_tokens_card`] and [`draw_agents_card`]
//!   into the rect they are given and return the rows they actually used.
//! - Data comes in through [`TokenSnapshot`] / [`AgentRow`], built from
//!   `TuiState` by [`token_snapshot`] / [`agent_rows`].
//! - Clickable rows are published with [`record_agent_hit_rows`] and read by the
//!   mouse handler with [`agent_at`].

use super::TuiState;
use ratatui::prelude::*;

/// Sidebar width in columns (border column included).
pub(crate) const SIDEBAR_WIDTH: u16 = 26;
/// Narrowest chat area that still gets the sidebar.
pub(crate) const SIDEBAR_MIN_TOTAL_WIDTH: u16 = 110;
/// Narrowest chat area that gets the cockpit at all (header/composer/footer).
pub(crate) const COCKPIT_MIN_WIDTH: u16 = 60;
/// Shortest chat area that gets the cockpit at all.
pub(crate) const COCKPIT_MIN_HEIGHT: u16 = 16;

/// Regions of the cockpit layout. All rects are inside the chat area passed
/// to [`cockpit_layout`]. `sidebar` is `None` on narrow terminals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CockpitRects {
    /// One-row brand header across the full width.
    pub header: Rect,
    /// Transcript + composer column (left of the sidebar).
    pub main: Rect,
    /// Right sidebar column, including its 1-column left border.
    pub sidebar: Option<Rect>,
    /// One-row footer across the full width.
    pub footer: Rect,
}

/// Decide the cockpit regions for `area`, or `None` when the terminal is too
/// small (caller falls back to the classic jcode layout).
pub(crate) fn cockpit_layout(area: Rect) -> Option<CockpitRects> {
    if area.width < COCKPIT_MIN_WIDTH || area.height < COCKPIT_MIN_HEIGHT {
        return None;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    if area.width >= SIDEBAR_MIN_TOTAL_WIDTH {
        let main = Rect::new(body.x, body.y, body.width - SIDEBAR_WIDTH, body.height);
        let sidebar = Rect::new(main.right(), body.y, SIDEBAR_WIDTH, body.height);
        Some(CockpitRects { header, main, sidebar: Some(sidebar), footer })
    } else {
        Some(CockpitRects { header, main: body, sidebar: None, footer })
    }
}

/// Whether the cockpit layout is wanted for this state (hero style active,
/// cockpit enabled in config, conversation started). Layout may still fall
/// back when [`cockpit_layout`] returns `None`.
pub(crate) fn cockpit_wanted(state: &dyn TuiState) -> bool {
    super::welcome::corner_sprite_wanted(state)
}

/// Shorten a working directory for display: last path component only, never
/// the full path (privacy + space). `C:\Users\dm101\TUI` -> `TUI`.
pub(crate) fn short_dir(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

// ------------------------------------------------------------ tokens card ---

/// Everything the tokens card shows. `None` means unknown (render "-"), never
/// zero. Owned by lane "tokens-card".
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TokenSnapshot {
    pub session_input: Option<u64>,
    pub session_output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    /// Session cost in USD, when the provider is metered.
    pub session_cost_usd: Option<f64>,
    pub context_used: Option<u64>,
    pub context_limit: Option<u64>,
    /// Live output tokens/sec while streaming.
    pub output_tps: Option<f32>,
    /// Sum of all swarm agents' context tokens (managed subtree).
    pub agents_context: Option<u64>,
    /// Sum of all swarm agents' output tokens.
    pub agents_output: Option<u64>,
    /// Number of managed swarm members, absent when there are none.
    pub agents_count: Option<usize>,
    /// Subscription window utilization 0.0-1.0 (5h / 7d), when known.
    pub window_5h: Option<f32>,
    pub window_7d: Option<f32>,
}

/// Build the token snapshot from state. Must be cheap: no I/O, no locks held
/// across frames. Owned by lane "tokens-card".
pub(crate) fn token_snapshot(state: &dyn TuiState) -> TokenSnapshot {
    // App supplies just resident token metadata, avoiding the full info widget's
    // git/diagram/background gathering on every animated sidebar frame.
    let lightweight = state.cockpit_token_data();
    let has_lightweight = lightweight.is_some();
    let data = lightweight.unwrap_or_else(|| state.info_widget_data());
    let streaming = matches!(state.status(), super::ProcessingStatus::Streaming);
    let usage = data.usage_info.as_ref().filter(|usage| usage.available);
    let totals = usage
        .filter(|usage| usage.input_tokens > 0 || usage.output_tokens > 0)
        .map(|usage| (usage.input_tokens, usage.output_tokens))
        .or_else(|| state.total_session_tokens());
    // Unlike authoritative session totals, live zero counters also mean
    // "not reported yet", so do not turn an absent half into a fake zero.
    let live = state.streaming_tokens();
    let cache = data.cache_hit_info.as_ref();
    let live_cache = state.streaming_cache_tokens();
    let mut snap = TokenSnapshot {
        session_input: totals.map(|tokens| tokens.0).or_else(|| (live.0 > 0).then_some(live.0)),
        session_output: totals.map(|tokens| tokens.1).or_else(|| (live.1 > 0).then_some(live.1)),
        cache_read: cache.map(|cache| cache.read_tokens)
            .or_else(|| usage.and_then(|usage| usage.cache_read_tokens))
            .or(live_cache.0),
        cache_write: cache.map(|cache| cache.creation_tokens)
            .or_else(|| usage.and_then(|usage| usage.cache_write_tokens))
            .or(live_cache.1),
        session_cost_usd: usage
            .filter(|usage| usage.provider == super::info_widget::UsageProvider::CostBased)
            .map(|usage| f64::from(usage.total_cost))
            .filter(|cost| cost.is_finite() && *cost >= 0.0),
        context_used: data.observed_context_tokens.or_else(|| {
            (!data.context_info_stale).then_some(data.context_info.as_ref())
                .flatten().filter(|info| info.total_chars > 0)
                .map(|info| info.estimated_tokens() as u64)
        }).or_else(|| {
            // Generic states may expose context only through TuiState. App's
            // lightweight path deliberately never rescans the transcript here.
            if has_lightweight { return None; }
            let context = state.context_snapshot();
            context.info.filter(|info| context.fresh && info.total_chars > 0)
                .map(|info| info.estimated_tokens() as u64)
        }),
        context_limit: data.context_limit.or_else(|| state.context_limit())
            .filter(|limit| *limit > 0).map(|limit| limit as u64),
        output_tps: streaming.then(|| state.output_tps()
            .or(data.tokens_per_second).or_else(|| usage.and_then(|usage| usage.output_tps)))
            .flatten().filter(|tps| tps.is_finite() && *tps >= 0.0),
        ..Default::default()
    };
    if let Some(usage) = usage.filter(|usage| matches!(usage.provider,
        super::info_widget::UsageProvider::Anthropic | super::info_widget::UsageProvider::OpenAI)) {
        // Labels distinguish a real zero-utilization window from an absent
        // window, whose legacy UsageInfo scalar is also zero.
        snap.window_5h = usage.primary_limit_label.as_ref().map(|_| usage.five_hour)
            .filter(|ratio| ratio.is_finite() && *ratio >= 0.0);
        snap.window_7d = usage.secondary_limit_label.as_ref().map(|_| usage.seven_day)
            .filter(|ratio| ratio.is_finite() && *ratio >= 0.0);
    }
    let (count, context, output) = state.cockpit_swarm_token_totals().unwrap_or_else(|| {
        let members = state.inline_swarm_members();
        // A partial sum would misleadingly understate swarm usage.
        let context = members.iter().try_fold(0u64, |sum, member| {
            member.runtime.context_tokens.map(|tokens| sum.saturating_add(tokens))
        });
        let output = members.iter().try_fold(0u64, |sum, member| {
            member.runtime.output_tokens.map(|tokens| sum.saturating_add(tokens))
        });
        (members.len(), context, output)
    });
    if count > 0 {
        snap.agents_count = Some(count);
        snap.agents_context = context;
        snap.agents_output = output;
    }
    snap
}

/// Draw the tokens card into `area`; return rows used. Owned by "tokens-card".
pub(crate) fn draw_tokens_card(buf: &mut Buffer, area: Rect, snap: &TokenSnapshot) -> u16 {
    use ratatui::widgets::{Block, BorderType, Borders, Clear, Widget};
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return 0;
    }
    let cost = snap.session_cost_usd.filter(|cost| cost.is_finite() && *cost >= 0.0);
    let tps = snap.output_tps.filter(|tps| tps.is_finite() && *tps >= 0.0);
    let available = [true, true, cost.is_some() || tps.is_some(), true,
        snap.window_5h.is_some() || snap.window_7d.is_some(), snap.agents_count.is_some()];
    // Preserve input/output and context first, then cache, cost, quotas, agents.
    let mut selected = [false; 6];
    let mut budget = area.height.saturating_sub(2) as usize;
    for index in [0, 3, 1, 2, 4, 5] {
        if budget > 0 && available[index] {
            selected[index] = true;
            budget -= 1;
        }
    }
    let rows = (selected.iter().filter(|selected| **selected).count() as u16 + 2)
        .min(area.height);
    let card = Rect::new(area.x, area.y, area.width, rows);
    Clear.render(card, buf);
    let border_style = Style::default().fg(Color::Rgb(180, 192, 255)).add_modifier(Modifier::DIM);
    let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
        .title(" tokens ").border_style(border_style).title_style(border_style);
    let inner = block.inner(card);
    block.render(card, buf);
    if inner.is_empty() {
        return rows;
    }
    let style = Style::default().fg(Color::Rgb(203, 209, 233));
    let mut y = inner.y;
    for (index, show) in selected.into_iter().enumerate() {
        if !show { continue; }
        if index == 3 {
            draw_token_context(buf, Rect::new(inner.x, y, inner.width, 1), snap, style);
        } else {
            let text = match index {
                0 => format!("in {}  out {}", token_number(snap.session_input), token_number(snap.session_output)),
                1 => format!("cache r {} w {}", token_number(snap.cache_read), token_number(snap.cache_write)),
                2 => match (cost, tps) {
                    (Some(cost), Some(tps)) => format!("${cost:.2} · {} t/s", token_number(Some(tps.round() as u64))),
                    (Some(cost), None) => format!("${cost:.2}"),
                    (None, Some(tps)) => format!("{} t/s", token_number(Some(tps.round() as u64))),
                    _ => unreachable!(),
                },
                4 => format!("5h {}  7d {}", token_percent(snap.window_5h), token_percent(snap.window_7d)),
                5 => {
                    let count = token_number(snap.agents_count.map(|count| count as u64));
                    match (snap.agents_context, snap.agents_output) {
                        (None, Some(output)) => format!("agents {count} · {} out", token_number(Some(output))),
                        (context, _) => format!("agents {count} · {} ctx", token_number(context)),
                    }
                },
                _ => unreachable!(),
            };
            buf.set_stringn(inner.x, y, text, inner.width as usize, style);
        }
        y += 1;
    }
    rows
}

fn token_number(value: Option<u64>) -> String {
    let Some(value) = value else { return "-".into(); };
    let (divisor, suffix) = match value {
        0..=999 => return value.to_string(),
        1_000..=999_949 => (1_000.0, "k"),
        999_950..=999_949_999 => (1_000_000.0, "M"),
        999_950_000..=999_949_999_999 => (1_000_000_000.0, "B"),
        999_950_000_000..=999_949_999_999_999 => (1_000_000_000_000.0, "T"),
        999_950_000_000_000..=999_949_999_999_999_999 => (1_000_000_000_000_000.0, "P"),
        _ => (1_000_000_000_000_000_000.0, "E"),
    };
    let number = value as f64 / divisor;
    if number >= 100.0 || (number * 10.0).round() % 10.0 == 0.0 { format!("{number:.0}{suffix}") }
    else { format!("{number:.1}{suffix}") }
}

fn token_percent(ratio: Option<f32>) -> String {
    ratio.filter(|ratio| ratio.is_finite() && *ratio >= 0.0)
        .map(|ratio| format!("{:.0}%", (ratio * 100.0).min(999.0)))
        .unwrap_or_else(|| "-".into())
}

const TOKEN_CONTEXT_WARNING: Color = Color::Rgb(255, 110, 130);

fn draw_token_context(buf: &mut Buffer, area: Rect, snap: &TokenSnapshot, style: Style) {
    let ratio = snap.context_used.zip(snap.context_limit.filter(|limit| *limit > 0))
        .map(|(used, limit)| used as f32 / limit as f32);
    let percent = token_percent(ratio);
    let width = area.width as usize;
    let bar_width = width.saturating_sub(5 + percent.len()).min(9);
    if bar_width == 0 {
        buf.set_stringn(area.x, area.y, format!("ctx {percent}"), width, style);
        return;
    }
    buf.set_stringn(area.x, area.y, "ctx ", width, style);
    let filled = ratio.map(|ratio| (ratio.clamp(0.0, 1.0) * bar_width as f32).round() as usize);
    for cell in 0..bar_width {
        let active = filled.is_some_and(|filled| cell < filled);
        let color = if ratio.is_some_and(|ratio| ratio > 0.85) {
            TOKEN_CONTEXT_WARNING
        } else {
            let step = cell as u16 * 100 / bar_width.saturating_sub(1).max(1) as u16;
            Color::Rgb((95 + step * 85 / 100) as u8, (157 + step * 35 / 100) as u8, 255)
        };
        let symbol = if filled.is_none() { "-" } else if active { "▓" } else { "░" };
        buf.set_string(area.x + 4 + cell as u16, area.y, symbol,
            Style::default().fg(color).add_modifier(if active { Modifier::empty() } else { Modifier::DIM }));
    }
    buf.set_stringn(area.x + 4 + bar_width as u16, area.y, format!(" {percent}"),
        width.saturating_sub(4 + bar_width), style);
}

// ------------------------------------------------------------ agents card ---

/// One row of the agents card. Owned by lane "agents-card".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentRow {
    pub session_id: String,
    pub name: String,
    pub status: String,
    pub task: Option<String>,
    pub headless: bool,
    pub todo_progress: Option<(u32, u32)>,
    pub output_tail: Option<String>,
    pub next_todo: Option<String>,
}

/// Build agent rows from state (display order). Owned by "agents-card".
pub(crate) fn agent_rows(state: &dyn TuiState) -> Vec<AgentRow> {
    let members = state.inline_swarm_members();
    let order = super::info_widget::swarm_gallery::members_display_order(&members);
    let by_id: std::collections::HashMap<_, _> = members
        .iter()
        .map(|member| (member.session_id.as_str(), member))
        .collect();
    order
        .iter()
        .filter_map(|id| by_id.get(id.as_str()))
        .map(|member| AgentRow {
            session_id: member.session_id.clone(),
            name: member.friendly_name.clone().unwrap_or_else(|| {
                member.session_id.chars().take(8).collect()
            }),
            status: if jcode_tui_render::swarm_gallery::is_active_status(&member.status)
                && member.runtime.wait_reason.is_some() {
                "waiting".to_string()
            } else { member.status.clone() },
            task: member.task_label.as_ref().filter(|task| !task.trim().is_empty())
                .or(member.detail.as_ref()).cloned(),
            headless: member.is_headless.unwrap_or(false),
            todo_progress: member.todo_progress,
            output_tail: member.output_tail.clone(),
            next_todo: member.todo_items.iter()
                .find(|todo| todo.status == "in_progress")
                .or_else(|| member.todo_items.iter().find(|todo| {
                    !matches!(todo.status.as_str(), "completed" | "cancelled")
                }))
                .map(|todo| todo.content.clone()),
        })
        .collect()
}

/// Draw the agents card into `area`; return rows used. `expanded` is the
/// session id whose live tail/todos are shown inline. Owned by "agents-card".
pub(crate) fn draw_agents_card(
    buf: &mut Buffer,
    area: Rect,
    rows: &[AgentRow],
    expanded: Option<&str>,
) -> u16 {
    use ratatui::widgets::{Block, BorderType, Borders, Widget};
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    // Bound output work to the few cells that can actually be shown. Do not
    // markdown-render the worker's live stream on the frame thread.
    fn clipped(text: &str, width: usize) -> String {
        let mut used = 0;
        text.chars().take_while(|ch| {
            if ch.is_control() { return false; }
            used += ch.width().unwrap_or(0);
            used <= width
        }).collect()
    }
    fn tail_lines(text: &str, width: usize, limit: usize) -> Vec<String> {
        let mut lines = std::collections::VecDeque::with_capacity(limit);
        // Only the most recent portion is useful for a three-line preview.
        let start = text.char_indices().rev().nth(width.saturating_mul(limit))
            .map(|(idx, _)| idx).unwrap_or(0);
        for line in text[start..].lines() {
            let mut rest = line;
            while !rest.is_empty() {
                let part = clipped(rest, width);
                if part.is_empty() { break; }
                rest = &rest[part.len()..];
                if lines.len() == limit { lines.pop_front(); }
                lines.push_back(part);
            }
        }
        lines.into_iter().collect()
    }

    if rows.is_empty() || area.width < 5 || area.height < 3 {
        record_agent_hit_rows(Vec::new());
        return 0;
    }
    let inner_width = area.width.saturating_sub(2) as usize;
    let dim = Style::default().fg(Color::DarkGray);
    let mut body: Vec<(Line<'static>, Option<&str>)> = Vec::new();
    let capacity = area.height.saturating_sub(2) as usize;
    for (index, row) in rows.iter().enumerate() {
        if body.len() == capacity { break; }
        let (dot, color) = match row.status.as_str() {
            "completed" | "done" => ("✓", Color::Green),
            "failed" | "error" => ("✗", Color::Red),
            "waiting" | "blocked" | "ready" | "idle" | "stopped" => ("◐", Color::Yellow),
            _ => ("●", Color::Blue),
        };
        let progress = row.todo_progress.map(|(done, total)| format!(" {done}/{total}"))
            .unwrap_or_default();
        let suffix_width = progress.width() + usize::from(row.headless) * 2;
        let label_width = inner_width.saturating_sub(2 + suffix_width);
        let name = clipped(&row.name, label_width);
        let remaining = label_width.saturating_sub(name.width());
        let task = row.task.as_deref().filter(|_| remaining > 2)
            .map(|task| format!(" {}", clipped(task, remaining - 1)))
            .unwrap_or_default();
        let padding = " ".repeat(label_width.saturating_sub(name.width() + task.width()));
        let name_style = if expanded == Some(row.session_id.as_str()) {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(Color::White) };
        body.push((Line::from(vec![
            Span::styled(format!("{dot} "), Style::default().fg(color)),
            Span::styled(name, name_style),
            Span::styled(task, dim),
            Span::raw(padding),
            Span::styled(progress, Style::default().fg(Color::Gray)),
            Span::styled(if row.headless { " h" } else { "" }, dim),
        ]), Some(row.session_id.as_str())));
        if expanded != Some(row.session_id.as_str()) { continue; }

        // Reserve a summary row for subsequent agents before spending spare
        // space on output, todo and hint rows.
        let spare = capacity.saturating_sub(body.len())
            .saturating_sub(rows.len().saturating_sub(index + 1));
        if spare == 0 { continue; }
        let todo_slots = usize::from(row.next_todo.is_some() && spare >= 2);
        let output_slots = spare.saturating_sub(1 + todo_slots).min(3);
        if output_slots > 0 {
            for line in tail_lines(row.output_tail.as_deref().unwrap_or(""), inner_width.saturating_sub(1), output_slots) {
                body.push((Line::styled(format!(" {line}"), Style::default().fg(Color::Gray)), None));
            }
        }
        if todo_slots > 0 {
            let todo = clipped(row.next_todo.as_deref().unwrap_or(""), inner_width.saturating_sub(3));
            body.push((Line::styled(format!(" › {todo}"), dim), None));
        }
        body.push((Line::styled(clipped(" dbl-click: open", inner_width), dim), None));
    }
    let used = (body.len() as u16).saturating_add(2);
    let card = Rect::new(area.x, area.y, area.width, used);
    Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(dim).title(" agents ").render(card, buf);
    let mut hits = Vec::with_capacity(rows.len().min(capacity));
    for (offset, (line, id)) in body.iter().enumerate() {
        let y = area.y + 1 + offset as u16;
        buf.set_line(area.x + 1, y, line, inner_width as u16);
        if let Some(id) = id {
            hits.push((y, area.x + 1, area.right() - 1, (*id).to_string()));
        }
    }
    record_agent_hit_rows(hits);
    used
}

/// Clickable agent rows from the last frame: (screen row y, x range, session id).
static AGENT_HIT_ROWS: std::sync::Mutex<Vec<(u16, u16, u16, String)>> =
    std::sync::Mutex::new(Vec::new());

/// Publish clickable agent rows for the mouse handler. Called by the agents
/// card each frame (empty vec when the card is not drawn).
pub(crate) fn record_agent_hit_rows(rows: Vec<(u16, u16, u16, String)>) {
    if let Ok(mut slot) = AGENT_HIT_ROWS.lock() {
        *slot = rows;
    }
}

/// Session id of the agent row at screen cell (x, y), if any.
pub(crate) fn agent_at(x: u16, y: u16) -> Option<String> {
    let slot = AGENT_HIT_ROWS.lock().ok()?;
    slot.iter()
        .find(|(row, x0, x1, _)| *row == y && x >= *x0 && x < *x1)
        .map(|(_, _, _, id)| id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_splits_sidebar_only_when_wide() {
        let wide = cockpit_layout(Rect::new(0, 0, 140, 40)).unwrap();
        let sidebar = wide.sidebar.unwrap();
        assert_eq!(wide.header.height, 1);
        assert_eq!(wide.footer.y, 39);
        assert_eq!(wide.main.right(), sidebar.x);
        assert_eq!(sidebar.right(), 140);
        let narrow = cockpit_layout(Rect::new(0, 0, 90, 30)).unwrap();
        assert!(narrow.sidebar.is_none());
        assert_eq!(narrow.main.width, 90);
        assert!(cockpit_layout(Rect::new(0, 0, 50, 30)).is_none());
        assert!(cockpit_layout(Rect::new(0, 0, 120, 10)).is_none());
    }

    #[test]
    fn short_dir_never_shows_full_path() {
        assert_eq!(short_dir(r"C:\Users\dm101\TUI"), "TUI");
        assert_eq!(short_dir("/home/me/proj/"), "proj");
        assert_eq!(short_dir("TUI"), "TUI");
    }

    #[test]
    fn agent_hit_rows_round_trip() {
        let _guard = agents_tests::hit_test_lock();
        record_agent_hit_rows(vec![(5, 100, 120, "s1".into())]);
        assert_eq!(agent_at(110, 5).as_deref(), Some("s1"));
        assert_eq!(agent_at(99, 5), None);
        assert_eq!(agent_at(110, 6), None);
        record_agent_hit_rows(Vec::new());
    }
}

#[cfg(test)]
mod agents_tests {
    use super::*;
    use crate::protocol::{SwarmMemberRuntime, SwarmMemberStatus, SwarmTodoItem};
    use crate::tui::app::App;
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    pub(super) fn hit_test_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn member(id: &str, name: &str, headless: bool) -> SwarmMemberStatus {
        SwarmMemberStatus {
            session_id: id.into(), friendly_name: Some(name.into()),
            status: "running".into(), detail: Some("fallback task".into()),
            task_label: Some("build".into()), role: Some("agent".into()),
            is_headless: Some(headless), live_attachments: Some(usize::from(!headless)),
            status_age_secs: Some(1), output_tail: Some("first line\nlive tail".into()),
            report_back_to_session_id: None, todo_progress: Some((4, 7)),
            todo_items: vec![
                SwarmTodoItem { content: "done".into(), status: "completed".into(), tool_intents: vec![] },
                SwarmTodoItem { content: "next test".into(), status: "in_progress".into(), tool_intents: vec![] },
            ],
            runtime: SwarmMemberRuntime::default(),
        }
    }

    fn row(name: &str, status: &str) -> AgentRow {
        AgentRow {
            session_id: name.into(), name: name.into(), status: status.into(),
            task: Some("build".into()), headless: true, todo_progress: Some((4, 7)),
            output_tail: Some("first line\nlive tail".into()), next_todo: Some("next test".into()),
        }
    }

    fn screen(buf: &Buffer) -> Vec<String> {
        (buf.area.y..buf.area.bottom()).map(|y| {
            (buf.area.x..buf.area.right()).map(|x| buf[(x, y)].symbol()).collect()
        }).collect()
    }

    #[test]
    fn rows_include_visible_and_headless_in_shortcut_order() {
        let mut coordinator = member("z", "root", false);
        coordinator.role = Some("coordinator".into());
        let members = vec![member("b", "fox", true), coordinator, member("a", "owl", false)];
        let expected = crate::tui::info_widget::swarm_gallery::members_display_order(&members);
        let app = App::cockpit_agents_test_app(members);
        let rows = agent_rows(&app);
        assert_eq!(rows.iter().map(|row| row.session_id.clone()).collect::<Vec<_>>(), expected);
        let fox = rows.iter().find(|row| row.name == "fox").unwrap();
        assert!(fox.headless);
        assert_eq!(fox.todo_progress, Some((4, 7)));
        assert_eq!(fox.next_todo.as_deref(), Some("next test"));
        assert_eq!(fox.task.as_deref(), Some("build"));
        assert_eq!(fox.output_tail.as_deref(), Some("first line\nlive tail"));
        assert!(!rows.iter().find(|row| row.name == "owl").unwrap().headless);
    }

    #[test]
    fn rows_use_detail_fallback_and_live_wait_status() {
        let mut waiting = member("waiting", "fox", true);
        waiting.task_label = None;
        waiting.friendly_name = None;
        waiting.runtime.wait_reason = Some("tool".into());
        waiting.todo_items[1].status = "pending".into();
        let app = App::cockpit_agents_test_app(vec![waiting]);
        let rows = agent_rows(&app);
        assert_eq!(rows[0].name, "waiting");
        assert_eq!(rows[0].status, "waiting");
        assert_eq!(rows[0].task.as_deref(), Some("fallback task"));
        assert_eq!(rows[0].next_todo.as_deref(), Some("next test"));
    }

    #[test]
    fn card_width_25_shows_names_status_progress_and_headless() {
        let _guard = hit_test_lock();
        let area = Rect::new(0, 0, 25, 12);
        let mut buf = Buffer::empty(area);
        let rows = [row("fox", "running"), row("owl", "waiting"), row("elk", "completed"), row("cat", "failed")];
        assert_eq!(draw_agents_card(&mut buf, area, &rows, None), 6);
        let lines = screen(&buf);
        assert!(lines[0].starts_with("╭ agents "));
        for (index, (name, dot, color)) in [
            ("fox", "●", Color::Blue), ("owl", "◐", Color::Yellow),
            ("elk", "✓", Color::Green), ("cat", "✗", Color::Red),
        ].iter().enumerate() {
            assert!(lines[index + 1].contains(name), "{:?}", lines);
            assert!(lines[index + 1].contains("4/7 h"));
            assert_eq!(buf[(1, index as u16 + 1)].symbol(), *dot);
            assert_eq!(buf[(1, index as u16 + 1)].fg, *color);
        }
    }

    #[test]
    fn expanded_card_shows_tail_next_todo_and_hint_inline() {
        let _guard = hit_test_lock();
        let area = Rect::new(0, 0, 25, 12);
        let mut buf = Buffer::empty(area);
        let used = draw_agents_card(&mut buf, area, &[row("fox", "running"), row("owl", "running")], Some("fox"));
        assert_eq!(used, 8);
        let lines = screen(&buf).join("\n");
        for text in ["first line", "live tail", "next test", "dbl-click: open", "owl"] {
            assert!(lines.contains(text), "missing {text}: {lines}");
        }
    }

    #[test]
    fn hit_rows_match_only_drawn_summary_rows_and_clear_on_empty() {
        let _guard = hit_test_lock();
        let area = Rect::new(5, 8, 25, 12);
        let mut buf = Buffer::empty(area);
        let rows = [row("fox", "running"), row("owl", "running")];
        draw_agents_card(&mut buf, area, &rows, Some("fox"));
        let hits = AGENT_HIT_ROWS.lock().unwrap().clone();
        assert_eq!(hits, vec![(9, 6, 29, "fox".into()), (14, 6, 29, "owl".into())]);
        assert_eq!(agent_at(6, 9).as_deref(), Some("fox"));
        assert_eq!(agent_at(28, 14).as_deref(), Some("owl"));
        assert_eq!(agent_at(5, 9), None);
        assert_eq!(agent_at(29, 9), None);
        assert_eq!(agent_at(6, 10), None);
        assert_eq!(draw_agents_card(&mut buf, area, &[], None), 0);
        assert_eq!(agent_at(6, 9), None);
    }

    #[test]
    fn constrained_card_reserves_agents_and_never_hits_hidden_rows() {
        let _guard = hit_test_lock();
        let rows = [row("fox", "running"), row("owl", "running"), row("elk", "running")];
        let area = Rect::new(0, 0, 25, 4);
        let mut buf = Buffer::empty(area);
        assert_eq!(draw_agents_card(&mut buf, area, &rows, Some("fox")), 4);
        let lines = screen(&buf);
        assert!(lines[1].contains("fox"));
        assert!(lines[2].contains("owl"));
        assert_eq!(agent_at(1, 3), None);
        assert_eq!(draw_agents_card(&mut buf, Rect::new(0, 0, 25, 2), &rows, None), 0);
        assert!(AGENT_HIT_ROWS.lock().unwrap().is_empty());
    }

    #[test]
    fn long_unicode_tail_is_bounded_wrapped_and_keeps_latest_text() {
        let _guard = hit_test_lock();
        let area = Rect::new(0, 0, 25, 12);
        let mut buf = Buffer::empty(area);
        let mut fox = row("狐fox", "running");
        fox.output_tail = Some(format!("{}\nlatest tail", "界".repeat(10000)));
        let used = draw_agents_card(&mut buf, area, &[fox], Some("狐fox"));
        assert!(used <= 8);
        let lines = screen(&buf);
        assert!(lines.iter().any(|line| line.contains("latest tail")));
        assert!(lines[used as usize - 1].ends_with('╯'));
    }

    #[test]
    fn click_single_toggles_and_double_dispatches_exact_agent_without_spawn() {
        let mut app = App::cockpit_agents_test_app(vec![member("visible", "owl", false), member("headless", "fox", true)]);
        let mut opened = Vec::new();
        let now = Instant::now();
        app.handle_cockpit_agent_click_with("headless", 5, now, |_, id| opened.push(id.to_string()));
        assert_eq!(app.cockpit_expanded_agent().as_deref(), Some("headless"));
        assert!(app.swarm_panel_focused());
        assert!(opened.is_empty());
        app.handle_cockpit_agent_click_with("headless", 5, now + Duration::from_millis(400), |_, id| opened.push(id.to_string()));
        assert_eq!(opened, ["headless"]);
        assert_eq!(app.cockpit_expanded_agent().as_deref(), Some("headless"));
        app.handle_cockpit_agent_click_with("headless", 5, now + Duration::from_secs(1), |_, _| panic!("not a double-click"));
        assert_eq!(app.cockpit_expanded_agent(), None);
        app.handle_cockpit_agent_click_with("visible", 6, now + Duration::from_secs(2), |_, _| panic!("first click"));
        app.handle_cockpit_agent_click_with("visible", 6, now + Duration::from_millis(2200), |_, id| opened.push(id.to_string()));
        assert_eq!(opened, ["headless", "visible"]);
    }

    #[test]
    fn different_row_or_agent_and_expired_click_do_not_pop_out() {
        let mut app = App::cockpit_agents_test_app(vec![member("a", "fox", true), member("b", "owl", false)]);
        let now = Instant::now();
        for (id, row, ms) in [("a", 5, 0), ("a", 6, 100), ("b", 6, 200), ("b", 6, 601)] {
            app.handle_cockpit_agent_click_with(id, row, now + Duration::from_millis(ms), |_, _| panic!("not a double-click"));
        }
        assert_eq!(app.cockpit_expanded_agent(), None);
        app.handle_cockpit_agent_click_with("missing", 6, now + Duration::from_millis(700), |_, _| panic!("stale agent"));
        assert_eq!(app.cockpit_expanded_agent(), None);
    }

    #[test]
    fn focused_enter_reuses_pop_out_for_visible_and_headless_agents() {
        use crossterm::event::{KeyCode, KeyModifiers};
        for headless in [false, true] {
            let mut app = App::cockpit_agents_test_app(vec![member("a", "fox", headless)]);
            assert!(!app.handle_swarm_panel_key(KeyCode::Enter, KeyModifiers::NONE));
            app.handle_cockpit_agent_click_with("a", 5, Instant::now(), |_, _| panic!("single click"));
            // The existing terminal launcher returns Ok(false) under cfg(test).
            assert!(app.handle_swarm_panel_key(KeyCode::Enter, KeyModifiers::NONE));
            assert!(app.status_notice().unwrap().contains("Could not open a terminal for fox"));
        }
    }
}

#[cfg(test)]
mod tokens_tests {
    use super::*;

    fn full_snapshot() -> TokenSnapshot {
        TokenSnapshot {
            session_input: Some(12_400), session_output: Some(3_100),
            cache_read: Some(9_800), cache_write: Some(1_200),
            session_cost_usd: Some(0.04), output_tps: Some(41.0),
            context_used: Some(21_000), context_limit: Some(100_000),
            window_5h: Some(0.34), window_7d: Some(0.12),
            agents_count: Some(2), agents_context: Some(48_000), agents_output: Some(6_000),
        }
    }

    fn text(buf: &Buffer) -> String {
        let mut text = String::new();
        for y in buf.area.y..buf.area.bottom() {
            for x in buf.area.x..buf.area.right() {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn full_data_fits_25_columns() {
        let area = Rect::new(0, 0, 25, 10);
        let mut buf = Buffer::empty(area);
        assert_eq!(draw_tokens_card(&mut buf, area, &full_snapshot()), 8);
        let text = text(&buf);
        for expected in ["╭ tokens ", "in 12.4k  out 3.1k", "cache r 9.8k w 1.2k",
            "$0.04 · 41 t/s", "21%", "5h 34%  7d 12%", "agents 2 · 48k ctx"] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
        assert_eq!(buf[(24, 7)].symbol(), "╯");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        let mut partial = full_snapshot();
        partial.agents_context = None;
        draw_tokens_card(&mut buf, area, &partial);
        assert!(self::text(&buf).contains("agents 2 · 6k out"));
    }

    #[test]
    fn unknown_values_are_dashes_never_fake_zero() {
        let area = Rect::new(0, 0, 25, 10);
        let mut buf = Buffer::empty(area);
        assert_eq!(draw_tokens_card(&mut buf, area, &TokenSnapshot::default()), 5);
        let text = text(&buf);
        assert!(text.contains("in -  out -"));
        assert!(text.contains("cache r - w -"));
        assert!(text.contains("ctx --------- -"));
        assert!(!text.contains('0'));
        assert!(!text.contains("t/s"));
        assert!(!text.contains('$'));
        assert!(!text.contains("agents"));
        assert!(!text.contains("5h"));
    }

    #[test]
    fn tiny_height_keeps_high_priority_lines_without_panicking() {
        for height in 0..=9 {
            for width in 0..=25 {
                let area = Rect::new(0, 0, width, height);
                let mut buf = Buffer::empty(area);
                assert!(draw_tokens_card(&mut buf, area, &full_snapshot()) <= height);
            }
        }
        let area = Rect::new(0, 0, 25, 4);
        let mut buf = Buffer::empty(area);
        assert_eq!(draw_tokens_card(&mut buf, area, &full_snapshot()), 4);
        let text = text(&buf);
        assert!(text.contains("in 12.4k"));
        assert!(text.contains("21%"));
        assert!(!text.contains("cache"));
        assert!(!text.contains("agents"));
    }

    #[test]
    fn context_over_85_percent_is_warning_colored() {
        let area = Rect::new(0, 0, 25, 8);
        let mut snap = full_snapshot();
        for (used, warning) in [(85_000, false), (85_001, true), (150_000, true)] {
            snap.context_used = Some(used);
            let mut buf = Buffer::empty(area);
            draw_tokens_card(&mut buf, area, &snap);
            let has_warning = buf.content.iter().any(|cell| cell.fg == TOKEN_CONTEXT_WARNING);
            assert_eq!(has_warning, warning, "context {used}");
        }
    }

    #[test]
    fn zero_limit_and_non_finite_values_do_not_render_fake_usage() {
        let area = Rect::new(0, 0, 25, 10);
        let mut snap = TokenSnapshot { context_used: Some(0), context_limit: Some(0),
            session_cost_usd: Some(f64::NAN), output_tps: Some(f32::INFINITY),
            window_5h: Some(0.0), ..Default::default() };
        let mut buf = Buffer::empty(area);
        draw_tokens_card(&mut buf, area, &snap);
        let rendered = text(&buf);
        assert!(rendered.contains("5h 0%  7d -"));
        assert!(!rendered.contains("NaN"));
        assert!(!rendered.contains("t/s"));
        snap.window_5h = None;
        draw_tokens_card(&mut buf, area, &snap);
        assert!(text(&buf).contains("ctx --------- -"));
    }

    #[test]
    fn clipped_offset_area_is_safe() {
        let mut buf = Buffer::empty(Rect::new(10, 10, 25, 8));
        assert_eq!(draw_tokens_card(&mut buf, Rect::new(8, 8, 25, 8), &full_snapshot()), 6);
        assert_eq!(draw_tokens_card(&mut buf, Rect::new(100, 100, 25, 8), &full_snapshot()), 0);
    }

    #[test]
    fn compact_numbers_keep_unknown_distinct_from_zero() {
        for (value, expected) in [(None, "-"), (Some(0), "0"), (Some(999), "999"),
            (Some(1_200), "1.2k"), (Some(3_400_000), "3.4M"), (Some(1_200_000_000), "1.2B"),
            (Some(u64::MAX), "18.4E")] {
            assert_eq!(token_number(value), expected);
        }
    }

    #[test]
    fn token_snapshot_empty_replay_keeps_unreported_usage_unknown() {
        // A real inert App exercises the lightweight accessor without a large
        // duplicate fake implementation of TuiState or credential/network work.
        let session = crate::session::Session::create(None, None);
        let app = crate::tui::App::new_for_replay_silent(session);
        let snap = token_snapshot(&app);
        assert_eq!(snap.session_input, None);
        assert_eq!(snap.session_output, None);
        assert_eq!(snap.session_cost_usd, None);
        assert_eq!(snap.cache_read, None);
        assert_eq!(snap.cache_write, None);
        assert_eq!(snap.output_tps, None);
        assert_eq!(snap.agents_count, None);
    }

    #[test]
    fn token_snapshot_sums_swarm_and_refreshes_counters_without_topology_change() {
        fn member(id: &str, context: Option<u64>, output: Option<u64>) -> crate::protocol::SwarmMemberStatus {
            serde_json::from_value(serde_json::json!({
                "session_id": id, "status": "running",
                "runtime": {"context_tokens": context, "output_tokens": output}
            })).unwrap()
        }
        let app = crate::tui::App::cockpit_agents_test_app(vec![
            member("a", Some(20_000), Some(3_000)), member("b", Some(28_000), Some(4_000))]);
        let snap = token_snapshot(&app);
        assert_eq!(snap.agents_count, Some(2));
        assert_eq!(snap.agents_context, Some(48_000));
        assert_eq!(snap.agents_output, Some(7_000));

        // Same IDs and parent edges reuse cached indices but must read fresh
        // counters, and must not substitute zero for an older producer's None.
        let app = crate::tui::App::cockpit_agents_test_app(vec![
            member("a", Some(22_000), Some(5_000)), member("b", None, Some(6_000))]);
        let snap = token_snapshot(&app);
        assert_eq!(snap.agents_context, None);
        assert_eq!(snap.agents_output, Some(11_000));

        let app = crate::tui::App::cockpit_agents_test_app(vec![member("c", Some(1), None)]);
        let snap = token_snapshot(&app);
        assert_eq!(snap.agents_count, Some(1));
        assert_eq!(snap.agents_context, Some(1));
        assert_eq!(snap.agents_output, None);
    }
}
