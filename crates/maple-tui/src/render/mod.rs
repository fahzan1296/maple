//! Render helpers: hyperlinks, output blocks, tool cards, tree/file lists,
//! Reference: `l/src/render/index.ts`.
//! width-aware text, code/output cells, link hints, extension links, status
//! line.

pub mod code_cell;
pub mod extension_links;
pub mod composite;
pub mod file_list;
pub mod hanging_wrap;
pub mod hyperlink;
pub mod link_hints;
pub mod output_block;
pub mod output_pane;
pub mod render_utils;
pub mod row_moves;
pub mod sixel;
pub mod status_line;
pub mod tool_card;
pub mod tree_list;
pub mod types;
pub mod width_aware_text;
