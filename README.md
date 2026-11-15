<!--
  maple — README
  Structured after the awesome-readme-template layout (see Acknowledgements).
  Clean-room project: no code is copied from the referenced repositories.
-->

<div align="center">

  <h1>maple</h1>

  <p>
    RAM-efficient, multi-session TUI coding agent.<br />
    Pure Rust · one binary · clean-room.
  </p>

  <p>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="license" /></a>
    <a href="Cargo.toml"><img src="https://img.shields.io/badge/rust-1.85%2B-orange" alt="rust" /></a>
    <img src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS-lightgrey" alt="platform" />
  </p>

  <h4>
    <a href="PRD.md">Documentation</a>
    <span> · </span>
    <a href="CHANGELOG.md">Changelog</a>
    <span> · </span>
    <a href="SECURITY.md">Security</a>
    <span> · </span>
    <a href="KONTRAK.md">Governance</a>
  </h4>
</div>

<br />

# :notebook_with_decorative_cover: Table of Contents

- [About the Project](#star2-about-the-project)
  * [Screenshots](#camera-screenshots)
  * [Tech Stack](#space_invader-tech-stack)
  * [Features](#dart-features)
  * [Color Reference](#art-color-reference)
  * [Environment Variables](#key-environment-variables)
- [Getting Started](#toolbox-getting-started)
  * [Prerequisites](#bangbang-prerequisites)
  * [Installation](#gear-installation)
  * [Running Tests](#test_tube-running-tests)
  * [Run Locally](#running-run-locally)
- [Usage](#eyes-usage)
- [Roadmap](#compass-roadmap)
- [Contributing](#wave-contributing)
  * [Code of Conduct](#scroll-code-of-conduct)
- [FAQ](#grey_question-faq)
- [License](#warning-license)
- [Contact](#handshake-contact)
- [Acknowledgements](#gem-acknowledgements)

# :star2: About the Project

maple is a TUI coding agent built around three distinctions:

1. **RAM-efficient** — pure Rust, one binary (no JS runtime), append-only
   transcripts with bounded scans, blob spill for large artifacts, and a
   per-session memory budget that is monitored and enforced (abort at 80%).
2. **Multi-session collaboration, independent sessions** — daemon-free,
   file-based mesh with flat peers (no host/guest), wire-compatible with the
   `.pi/messenger` format used by pi-messenger.
3. **omp-class UX + resource discipline** — differential renderer, Agent Hub,
   `ask`, keybindings, theme, hashline edits, two-phase autolearn, and
   pre-flight/monitor/abort enforced by the runtime, not just asked of the
   agent.

maple is a **clean-room implementation**: it is written from scratch and
inspired by [oh-my-pi](https://github.com/) (TUI/UX, session, edit, memory)
and pi-messenger (flat cross-session mesh). No code is copied from either —
see `THIRD-PARTY-NOTICES`.

## :camera: Screenshots

The TUI is four regions: header, transcript, input, statusline.

```text
╭─ maple ─ 01a1de21 ──────────────────────────────────────────────────────────╮
│ cwd    /home/Fahzan/maple                                                 │
│ model  unknown                                                           │
│ pid    4242                                                              │
╰────────────────────────────────────────────────────────────────────────────╯
╭─ transcript ──────────────────────────────────────────────────────────────╮
│ [2026-11-15T07:34:42] message  hello tui                                 │
│ [2026-11-15T07:35:01] message  second line                                │
│                                                                       ▲   │
╰────────────────────────────────────────────────────────────────────────────╯
╭─ input  (i edit) ──────────────────────────────────────────────────────────╮
│ type a message…                                                          │
╰────────────────────────────────────────────────────────────────────────────╯
 q quit · i input · n new session · j/k scroll                    2 entries
```

## :space_invader: Tech Stack

<details>
  <summary>Core</summary>
  <ul>
    <li><a href="https://www.rust-lang.org/">Rust 1.85+</a> — single static-ish binary</li>
    <li><a href="https://crates.io/crates/ratatui">ratatui</a> — terminal renderer (differential buffer)</li>
    <li><a href="https://crates.io/crates/crossterm">crossterm</a> — raw mode + input events</li>
    <li><a href="https://crates.io/crates/clap">clap</a> — CLI</li>
    <li><a href="https://crates.io/crates/serde">serde</a> / <a href="https://crates.io/crates/serde_json">serde_json</a> — transcript schema</li>
    <li><a href="https://crates.io/crates/sha2">sha2</a> — blob content addressing</li>
    <li><a href="https://crates.io/crates/uuid">uuid</a> — v7 session/entry ids</li>
    <li><a href="https://crates.io/crates/time">time</a> — RFC 3339 timestamps</li>
  </ul>
</details>

<details>
  <summary>Storage</summary>
  <ul>
    <li>Append-only JSONL transcripts, one file per session</li>
    <li>Content-addressed blob spill (SHA-256)</li>
    <li>No database, no daemon</li>
  </ul>
</details>

## :dart: Features

- `maple` — TUI session for the current directory.
- `maple headless` — create/load a session and print a JSON summary.
- `maple list` — list sessions for the current directory.
- `--resume <id-or-stem>` — reopen any session.
- Branching with `parentSession` lineage that never blocks either side.
- Bounded scan: sessions ≥ 8 MiB are read as header + tail only.
- Blob spill for large artifacts, addressed by SHA-256.
- One-writer lease per session file (stale locks are reclaimed).

## :art: Color Reference

maple uses terminal-native colors; the renderer only sets semantic accents.

| Role | Color |
| --- | --- |
| default text | terminal foreground |
| timestamps / hints | `dark gray` |
| active input border | `green` |

## :key: Environment Variables

| Variable | Meaning | Default |
| --- | --- | --- |
| `MAPLE_HOME` | Root agent state directory | `~/.maple` |
| `HOME` | Fallback when `MAPLE_HOME` is unset | — |

State layout:

```
~/.maple/agent/
├── sessions/<hex-cwd>/<ts>_<session-id>.jsonl   # append-only transcript
└── blobs/<sha256>                               # content-addressed spill
```

# :toolbox: Getting Started

## :bangbang: Prerequisites

- Rust 1.85 or newer (`cargo --version`)
- A terminal with Unicode support

## :gear: Installation

```bash
git clone <repository-url> maple
cd maple
cargo build --release
```

## :test_tube: Running Tests

```bash
cargo test
```

## :running: Run Locally

```bash
# TUI — creates a session for the current directory
./target/release/maple

# Headless — print a JSON session summary and exit
./target/release/maple headless --title "scratch"

# Resume a session by id or file stem
./target/release/maple --resume <id-or-stem>

# List sessions for this directory
./target/release/maple list
```

# :eyes: Usage

A transcript is one JSON object per line. The first line is the header:

```json
{"type":"session","version":3,"id":"01a1de21-…","timestamp":"2026-11-15T07:34:42.814Z","cwd":"/home/Fahzan/maple","title":"scratch","titleSource":"user"}
{"type":"message","id":"a1b2c3d4","timestamp":"2026-11-15T07:34:43.100Z","text":"hello tui"}
```

Keybindings:

| Key | Mode | Action |
| --- | --- | --- |
| `i` | normal | enter input mode |
| `Esc` | input | cancel back to normal |
| `Enter` | input | append the message to the transcript |
| `n` | normal | start a new independent session |
| `j` / `k` | normal | scroll the transcript |
| `q`, `Ctrl-C` | any | quit |

Sessions are peers: branching records `parentSession` for lineage only and
never blocks either side from writing.

# :compass: Roadmap

Implementation is incremental, in `PRD.md` §14 order (no phase gates).

* [x] CLI + session core
* [x] TUI renderer (minimal: header / transcript / input / statusline)
* [ ] Agent Hub (roster, inspector, focus/steer/revive/kill, `ask`)
* [ ] Mesh (`.pi/messenger` compat: registry, inbox+watch, feed, lock, hooks)
* [ ] Resource guard (pre-flight abort, monitoring, OOM protocol)
* [ ] Hashline edit (`[PATH#TAG]`, stale recovery, error classes)
* [ ] Autolearn (`learn`, `manage_skill`, two-phase pipeline)
* [ ] Crew (`plan` / `work` / `task.*`: graph, waves, claims)
* [ ] Hardening + docs (security pass, runbook, operations)

# :wave: Contributing

Work follows `KONTRAK.md` (AI Operating Contract v7.0): P1 coordination via
pi-messenger, P2 the work itself, aggressive resource limits (§SOP-RESLIM)
enforced at runtime. Open an issue before large changes.

## :scroll: Code of Conduct

Be precise, cite evidence, and keep the resource contract. See
`SECURITY.md` for reporting and `KONTRAK.md` for operating rules.

# :grey_question: FAQ

- **Is this a fork of oh-my-pi?**

  No. It is a clean-room implementation. See `docs/adr/0001-clean-room.md`
  and `THIRD-PARTY-NOTICES`.

- **Why pure Rust?**

  One binary, no JS runtime, and predictable RSS. See `docs/adr/0003-pure-rust.md`.

- **Does it run on Windows?**

  Not yet; Linux and macOS first (`PRD.md` §15).

- **Where is coordination state?**

  `.pi/messenger/` (project feed) and `~/.pi/agent/messenger/`
  (registry/inbox). See `PRD.md` §7.

# :warning: License

MIT — see `LICENSE` and `THIRD-PARTY-NOTICES`.

# :handshake: Contact

Open an issue in this repository, or reach the maintainer through the
project's pi-messenger feed.

# :gem: Acknowledgements

- [oh-my-pi](https://github.com/) — TUI/UX, session, edit, and memory design
  reference (read-only).
- [pi-messenger](https://github.com/) — flat cross-session mesh reference
  (read-only).
- [awesome-readme-template](https://github.com/Louis3797/awesome-readme-template) —
  README structure.
