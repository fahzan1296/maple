# ADR 0005 — Module naming standard

**Status:** Accepted
**Date:** 2026-11-19

## Context

`crates/maple-tui` grew by splitting over-long files into folder modules. Several
splits left names that describe how the split was made instead of what the code
does: `core1`/`core2`/`core3`, `state1`..`state4`, `component_a`/`component_b`,
`helpers_more`, `describe_parts`, `selector_more`, `misc`, `io_rx`, `mux`,
`key_hint_format`, `hotkeys_markdown`, `space_hold`. A reader cannot tell from
the name where a symbol lives or why.

## Decision

1. A module is named for the concept it owns, in lowercase `snake_case`, one
   noun or verb-noun phrase, no abbreviations, no numerals, no letter suffixes.
2. Split-mechanics suffixes are forbidden: `_more`, `_parts`, `_misc`, `_mux`,
   `_rx`, `_format`, `_matchers`, `core<N>`, `state<N>`, `*_a`/`*_b`, `helpers*`.
3. If a file outgrows the 150-code-SLOC ceiling, split by responsibility and
   give each part the responsibility as its name (`ranking.rs`, `preview.rs`,
   `keys.rs`), never by ordinal.
4. A folder module keeps `mod.rs` as the façade (docs, `mod` declarations,
   `pub use` re-exports); parts are implementation files named for their role.
5. Standard spellings are used even when a concept is Maple-specific: `io_rx` →
   `input_pipe`, `mux` → `terminal_mux`, `status_line` → `statusline`,
   `html_form` → `html_form` (kept: it is the concept, not a split artifact).
6. Renames land as their own mechanical pull request: file move, `mod`/`#[path]`
   update, re-export update, `cargo check`, no behavior change in the same diff.

## Applied map (mechanical, no behavior change)

| Old | New | Reason |
|---|---|---|
| `overlays/selector_more/` | `overlays/selector/` | no "extra selectors" folder; one concept |
| `overlays/misc/` | `overlays/panels/` | `misc` names nothing |
| `io_rx/` | `input_pipe/` | receives terminal input; not a channel detail |
| `mux/` | `terminal_mux/` | spelled out |
| `keymap/app_keybindings/` | `keymap/bindings/` | "app" is implied |
| `keymap/hotkeys_markdown/` | `keymap/hotkey_sheet/` | it renders the sheet, not markdown generally |
| `keymap/key_hint_format/` | `keymap/hints/` | one concept |
| `keymap/keybinding_matchers/` | `keymap/matchers/` | one concept |
| `editing/space_hold/` | `editing/hold_gesture/` | describes the gesture |
| `core<N>.rs`, `state<N>.rs`, `*_a.rs`, `*_b.rs`, `*_more.rs` | responsibility names | see rule 3 |

Renames are executable work tracked in the module-naming sweep; this ADR fixes
the standard so new splits do not reintroduce the artifacts.

## Alternatives

- **Keep the artifacts, document them in a map.** Rejected: the map rots and
  every lookup costs a second hop.
- **One flat file per module again.** Rejected: violates the 150-code-SLOC
  ceiling and makes diffs unreadable.
