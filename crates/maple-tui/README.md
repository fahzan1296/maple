# maple-tui

Clean-room Rust port of the `@oh-my-pi/pi-tui` terminal UI toolkit: a
component model with differential rendering, ANSI-aware text shaping, themes,
and terminal backends.

Pure Rust, `edition = "2021"`, `rust-version = 1.85`. No TypeScript code is
copied; behavior and wire formats are reimplemented.

## Layout

| Module | Responsibility |
| --- | --- |
| `component`, `components` | `Component` trait and built-in widgets (text, input, editor, markdown, lists, tables, charts, layout) |
| `tui`, `screen` | Differential renderer, synchronized output, UI loop |
| `terminal` | `Terminal` trait, process backend (raw mode, alt screen, paste, mouse, resize), virtual backend |
| `keys`, `keybindings`, `mouse`, `mouse_sgr` | Input decoding and key predicates |
| `style`, `text_shape`, `ansi_scan` | ANSI-aware styling, width measurement, wrapping |
| `theme` | JSON theme schema, color math, symbol presets, built-in dark/light themes |
| `render`, `chrome` | Output blocks, tool cards, hyperlinks, message/overlay rendering |
| `status_line` | Segment catalog, renderers, separators |
| `native` | TSP document state, bounded reconciler, image blobs, capability probes |
| `fuzzy`, `autocomplete`, `lang_from_path` | Search ranking, completion providers, language detection |
| `latex_to_unicode` | LaTeX math to Unicode/ANSI conversion |
| `glyph_protocol` | Glyph Protocol client and bundled icon registration |
| `windows_input` | Win32 input-mode record decoder/encoder |
| `multiplexers`, `ttyid`, `deccara` | Terminal multiplexer detection, TTY identity, DECCARA |
| `debug_server`, `desktop_notify`, `loop_watchdog`, `frame_rate`, `spinner_clock` | Diagnostics and runtime guards |

## Status

Implemented and unit-tested: core rendering loop, terminal backends, input
decoding, theme pipeline, the component catalog, status line, native TSP
reconciler, fuzzy matching, autocomplete, glyph protocol, and LaTeX math.

See `CHANGELOG.md` at the repository root for the release history.
