//! Glyph vocabulary shared by the hub frame, roster rows, and picker.

pub const CURSOR: &str = "❯";
pub const ARROW_RIGHT: &str = "›";
pub const SEPARATOR: &str = "─";

/// Session status markers.
pub const STATUS_ACTIVE: &str = "●";
pub const STATUS_BUSY: &str = "◐";
pub const STATUS_IDLE: &str = "○";
pub const STATUS_ERROR: &str = "✕";

/// Rounded box and split-pane chrome.
pub const BOX: [char; 6] = ['╭', '╮', '╰', '╯', '─', '│'];
pub const TEE_DOWN: char = '┬';
pub const TEE_UP: char = '┴';
pub const TEE_RIGHT: char = '├';
pub const TEE_LEFT: char = '┤';
pub const CROSS: char = '┼';

/// Tree rails (two columns wide so they align with the branch glyphs).
pub const TREE_BRANCH: &str = "├─";
pub const TREE_LAST: &str = "└─";
pub const TREE_VERT: &str = "│ ";
pub const TREE_SPACE: &str = "  ";
