# maple-tui port build log

## Round 5 — LIB GREEN
- `cargo build -p maple-tui` **rc=0** (242 s). Warnings only.

### Fixes that got lib green
- terminal/cursor_visibility.rs: `final` reserved keyword → `terminator`; removed dangling doc comment.
- terminal/mod.rs: `pub use cursor_visibility::CursorVisibilityTracker;`.
- terminal/process.rs: restored `worker: Option<JoinHandle<()>>`.
- render/render_utils.rs:1424 OsString → `into_string().ok()` then parse; :1526 `"{}{}"`; :2684 `cache.as_deref()`.
- screen.rs: import `visible_width`.
- tools/grouped_file_output.rs:271 `replacen(target, 1, "")`; :494 compute `skip` before move; `grouped_link_targets<'a>` + `+ 'a`.
- components/markdown.rs:635 `extension_link_scheme(&url)`; removed stray `}` that closed `impl Markdown` before `fast_tail_rows`, added `}` after it.
- tools/agent_tree.rs:325 `crate::thinking::parse_configured_thinking_level(Some(&level.0))`.
- overlays/model_hub.rs:3216 `Some(index) == self.role_hover`; `render_footer` uses `self.strip.take()`; `render_strip` → `&mut self`.
- tools/job_widget.rs: deref `*value`; `unwrap_or_default()`; `Props::insert` (no `set_prop` on Props); `figure_spans(&glance)`; `tile_visual(view.blocks.first())`; clone `failed`.
- utils.rs:394 `crate::render::render_utils::slice_by_column` (width-reporting variant).
- chat/assistant_message.rs: added `last_transient: bool` field + init; `renderer_cache.borrow_mut().clear()` ×2.

## Next: test build
- 306 integration tests in `crates/maple-tui/tests`. Build in chunks (avoid link OOM), jobs=1.
- `cargo test -p maple-tui --no-run --message-format=short --test ...`
