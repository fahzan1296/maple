//! Port target: `l/test/tool-execution-memoization.test.ts`.
//!
//! The TS contract asserts render-memoization: a `ToolExecutionComponent` must
//! call tool `renderResult`/`renderCall` exactly once per meaningful change and
//! never per `invalidate()`/frame flood, using an internal dirty-key guard.
//! It relies on injecting a renderer whose `renderResult`/`renderCall` are
//! observable (spy counters) and on the component reusing shaped output across
//! repaints.
//!
//! In current Rust (`chat::tool_execution::ToolExecutionComponent`), render/describe
//! recomputation is not exposed as a spyable memoization seam with the same
//! guarantees: there is no public `ToolViewRenderer` call-count contract,
//! `invalidate()` bumps `block_version`, and `render()`/native describe rebuild
//! views on demand without an equivalent documented dirty-key cache boundary.
//! Writing assertions against block versions or incidental recomputation would
//! not faithfully port the TS memoization contract (and cannot be verified here:
//! no cargo/rustc). This file records the gap explicitly.

#[test]
#[ignore = "missing facility: no observable renderResult/renderCall memoization seam or dirty-key render cache contract in Rust ToolExecutionComponent"]
fn reshapes_once_per_meaningful_change_not_per_invalidate_frame() {}

#[test]
#[ignore = "missing facility: streamed call-arg preview memoization/re-shape counting not exposed via portable Rust API"]
fn reshapes_call_preview_when_streamed_args_change() {}
