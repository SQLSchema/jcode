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
    /// Subscription window utilization 0.0-1.0 (5h / 7d), when known.
    pub window_5h: Option<f32>,
    pub window_7d: Option<f32>,
}

/// Build the token snapshot from state. Must be cheap: no I/O, no locks held
/// across frames. Owned by lane "tokens-card".
pub(crate) fn token_snapshot(_state: &dyn TuiState) -> TokenSnapshot {
    TokenSnapshot::default()
}

/// Draw the tokens card into `area`; return rows used. Owned by "tokens-card".
pub(crate) fn draw_tokens_card(_buf: &mut Buffer, _area: Rect, _snap: &TokenSnapshot) -> u16 {
    0
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
}

/// Build agent rows from state (display order). Owned by "agents-card".
pub(crate) fn agent_rows(_state: &dyn TuiState) -> Vec<AgentRow> {
    Vec::new()
}

/// Draw the agents card into `area`; return rows used. `expanded` is the
/// session id whose live tail/todos are shown inline. Owned by "agents-card".
pub(crate) fn draw_agents_card(
    _buf: &mut Buffer,
    _area: Rect,
    _rows: &[AgentRow],
    _expanded: Option<&str>,
) -> u16 {
    0
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
        record_agent_hit_rows(vec![(5, 100, 120, "s1".into())]);
        assert_eq!(agent_at(110, 5).as_deref(), Some("s1"));
        assert_eq!(agent_at(99, 5), None);
        assert_eq!(agent_at(110, 6), None);
        record_agent_hit_rows(Vec::new());
    }
}
