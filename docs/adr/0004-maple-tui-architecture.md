# 0004 — maple-tui crate architecture

Status: accepted

## Context

The reference TypeScript package (`l/`, `@oh-my-pi/pi-tui`) is a large TUI
toolkit: components, differential rendering, themes, terminal backends, native
surface reconciliation, and several text utilities. The Rust workspace needs a
single library crate that other `maple` crates can depend on without pulling in
the reference implementation.

## Decision

- One library crate at `crates/maple-tui`, no binary. The root `maple` package
  remains the agent binary and depends on it.
- Module boundaries follow the reference's public surface (`theme`, `render`,
  `chrome`, `components`, `status-line`, `tools`, `native`), but each module is
  implemented against Rust contracts: `Component` is a trait with
  `render(width) -> Vec<String>` and `handle_input(&Key)`, styles are values
  rather than closures, and themes are serde-typed JSON.
- Rendering is differential: `screen` diffs the previous buffer and emits only
  changed cells, with synchronized-output framing. Components never write to
  the terminal directly.
- Terminal access sits behind the `Terminal` trait with two implementations:
  `ProcessTerminal` (real TTY) and `VirtualTerminal` (tests and headless).
- Pure logic (key decoding, fuzzy scoring, LaTeX conversion, colors, wrapping)
  is I/O-free and unit-tested; side effects live at the edges.

## Consequences

- Consumers can render whole interfaces in a virtual terminal and assert on
  rows, which keeps UI tests deterministic.
- Adding a backend (e.g. a Windows console) means implementing `Terminal`, not
  touching components.
- The crate compiles without a TTY; integration tests need no terminal.

## Alternatives considered

- Direct `ratatui` rendering: rejected because the reference's differential
  cell model, synchronized output, and component contract are part of the
  behavior under test.
- One module per reference file: rejected as churn; related concerns are
  grouped (for example `style`, `text_shape`, and `ansi_scan`).
