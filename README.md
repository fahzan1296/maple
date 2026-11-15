<div align="center">

  <h1>maple</h1>

  <p>
    A RAM-efficient TUI coding agent with flat, independent multi-session
    collaboration.
  </p>

  <!-- Badges -->
  <p>
    <a href="https://github.com/Fahzan1296/maple/graphs/contributors">
      <img src="https://img.shields.io/github/contributors/Fahzan1296/maple" alt="contributors" />
    </a>
    <a href="https://github.com/Fahzan1296/maple/commits/master">
      <img src="https://img.shields.io/github/last-commit/Fahzan1296/maple" alt="last update" />
    </a>
    <a href="https://github.com/Fahzan1296/maple/network/members">
      <img src="https://img.shields.io/github/forks/Fahzan1296/maple" alt="forks" />
    </a>
    <a href="https://github.com/Fahzan1296/maple/stargazers">
      <img src="https://img.shields.io/github/stars/Fahzan1296/maple" alt="stars" />
    </a>
    <a href="https://github.com/Fahzan1296/maple/issues/">
      <img src="https://img.shields.io/github/issues/Fahzan1296/maple" alt="open issues" />
    </a>
    <a href="https://github.com/Fahzan1296/maple/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/Fahzan1296/maple.svg" alt="license" />
    </a>
  </p>

  <h4>
    <a href="#toolbox-getting-started">Get Started</a>
    <span> · </span>
    <a href="PRD.md">PRD &amp; Design</a>
    <span> · </span>
    <a href="KONTRAK.md">Operating Contract</a>
    <span> · </span>
    <a href="https://github.com/Fahzan1296/maple/issues/new/choose">Report Bug</a>
    <span> · </span>
    <a href="https://github.com/Fahzan1296/maple/issues/new/choose">Request Feature</a>
  </h4>
</div>

<br />

> **Status: v0.1 scaffold.** The design docs, governance contract, and ADRs are in
> place. The Rust implementation is being built incrementally in the order
> defined in [`PRD.md`](PRD.md) §14 — no feature is gated behind a phase, each
> step must be green (build + test) before the next.

<!-- Table of Contents -->
# :notebook_with_decorative_cover: Table of Contents

- [About the Project](#star2-about-the-project)
  * [Project Status](#bar_chart-project-status)
  * [Tech Stack](#space_invader-tech-stack)
  * [Features](#dart-features)
- [Getting Started](#toolbox-getting-started)
  * [Prerequisites](#bangbang-prerequisites)
  * [Installation](#gear-installation)
  * [Running Tests](#test_tube-running-tests)
  * [Run Locally](#running-run-locally)
  * [Deployment](#triangular_flag_on_post-deployment)
- [Usage](#eyes-usage)
- [Roadmap](#compass-roadmap)
- [Contributing](#wave-contributing)
  * [Code of Conduct](#scroll-code-of-conduct)
- [FAQ](#grey_question-faq)
- [License](#warning-license)
- [Contact](#handshake-contact)
- [Acknowledgements](#gem-acknowledgements)

<!-- About the Project -->
## :star2: About the Project

`maple` is a TUI coding agent built around three distinctions:

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

`maple` is a **clean-room implementation** ([ADR-0001](docs/adr/0001-clean-room.md)):
it is written from scratch and inspired by [oh-my-pi](https://github.com/can1357/oh-my-pi)
(TUI/UX, session, edit, memory) and
[pi-messenger](https://github.com/nicobailon/pi-messenger) (flat cross-session
mesh). No code is copied from either — see
[`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES).

<!-- Project Status -->
### :bar_chart: Project Status

Work is implemented directly, incrementally, in the build order below (user
decision, 2026-11-15: no M0–M6 phase gates).

| # | Feature | Status |
|---|---|---|
| 1 | CLI + session core | 🚧 in progress |
| 2 | TUI renderer | ⏳ pending |
| 3 | Agent Hub | ⏳ pending |
| 4 | Mesh (`.pi/messenger` compat) | ⏳ pending |
| 5 | Resource guard | ⏳ pending |
| 6 | Hashline edit | ⏳ pending |
| 7 | Autolearn | ⏳ pending |
| 8 | Crew (`plan`/`work`/`task.*`) | ⏳ pending |
| 9 | Hardening + docs | ⏳ pending |

<!-- TechStack -->
### :space_invader: Tech Stack

<details>
  <summary>Language</summary>
  <ul>
    <li><a href="https://www.rust-lang.org/">Rust</a> — one binary, no JS runtime (<a href="docs/adr/0003-pure-rust.md">ADR-0003</a>)</li>
  </ul>
</details>

<details>
  <summary>TUI</summary>
  <ul>
    <li><a href="https://ratatui.rs/">ratatui</a> — differential renderer: viewport diff + history batch</li>
    <li><a href="https://github.com/crossterm-rs/crossterm">crossterm</a> — terminal input/output</li>
  </ul>
</details>

<details>
<summary>Storage</summary>
  <ul>
    <li>Append-only <code>JSONL</code> transcripts in <code>~/.maple/agent/sessions/</code></li>
    <li><a href="https://www.sqlite.org/">SQLite</a> title index (<code>history.db</code>)</li>
    <li>Blob spill for large artifacts (<code>~/.maple/agent/blobs/</code>)</li>
  </ul>
</details>

<details>
<summary>Mesh</summary>
  <ul>
    <li>File-based <code>.pi/messenger/</code>: <code>registry/</code>, <code>inbox/</code>, <code>feed.jsonl</code>, <code>swarm.lock</code>, claims</li>
    <li>Wire-compatible with <a href="https://github.com/nicobailon/pi-messenger">pi-messenger</a></li>
  </ul>
</details>

<details>
<summary>Governance</summary>
  <ul>
    <li><code>KONTRAK.md</code> — AI Operating Contract v7.0</li>
  </ul>
</details>

<!-- Features -->
### :dart: Features

| Feature | What it does |
|---|---|
| Session core | One process = one session = one identity (`name`, `pid`, `sessionId`, `cwd`, `model`). JSONL tree with `leafId`, lease protection, bounded scan, blob spill. |
| Flat mesh | `join`/`leave`, `list`/`whois`, `send`, `broadcast`, `reserve`/`release`, `feed`, `team.memory`. No host/guest hierarchy, no daemon. |
| Resource guard | Pre-flight abort, monitoring ≥ 5 s, abort at 80% of any limit, OOM protocol. Enforced at runtime, not by convention. |
| Hashline edit | Content-hash anchor `[PATH#TAG]`, block ops, stale recovery, seen-line enforcement, explicit error classes. |
| Agent Hub | Roster + inspector (model, cost, tokens, active tool); focus/steer/revive/kill; `ask` overlay. |
| Autolearn | `learn`, `manage_skill`, two-phase extraction → consolidation, project-scoped, cross-session. |
| Crew | `plan`/`work`/`task.*`: dependency graph, parallel waves, claims and completions. |

<!-- Getting Started -->
## :toolbox: Getting Started

<!-- Prerequisites -->
### :bangbang: Prerequisites

- **Rust** stable toolchain (`rustc` + `cargo`) — see [rustup](https://rustup.rs/)
- **git**
- **Linux or macOS** (Windows support is a later goal)
- Build/test runs with a ≤ 512 MB budget; do not start a build while the host
  is already under memory pressure (see `KONTRAK.md` §SOP-RESLIM)

<!-- Installation -->
### :gear: Installation

Build from source:

```bash
git clone https://github.com/Fahzan1296/maple.git
cd maple
cargo build --release
```

The binary lands in `target/release/maple`.

<!-- Running Tests -->
### :test_tube: Running Tests

```bash
cargo test
```

<!-- Run Locally -->
### :running: Run Locally

Headless session smoke test (creates a JSONL session file):

```bash
cargo run -- --headless
```

Interactive TUI:

```bash
cargo run
```

<!-- Deployment -->
### :triangular_flag_on_post: Deployment

`maple` is a single binary with no daemon and no required services. Copy
`target/release/maple` onto the host; agent state lives under `~/.maple/` and
mesh state under `.pi/messenger/` (project-scoped). There is nothing to
provision beyond the binary.

<!-- Usage -->
## :eyes: Usage

The documented surface (see [`PRD.md`](PRD.md) §7–§9 for the full contract):

```text
# session
maple --headless                 # headless JSONL session
maple                            # interactive TUI

# mesh
maple list                       # peers + liveness
maple send <peer> "message"      # direct message
maple broadcast "message"        # to every peer inbox
maple feed                       # append-only project feed
maple reserve "src/auth/**" "refactor"
maple release

# memory / crew
maple learn "lesson text"
maple plan / work / task.*       # crew graph + parallel waves
```

<!-- Roadmap -->
## :compass: Roadmap

The authoritative roadmap is [`PRD.md`](PRD.md) §14 (build order) and the
status table under [Project Status](#bar_chart-project-status). Governance and
operational rules live in [`KONTRAK.md`](KONTRAK.md).

Open questions (naming, crew in v1 or later, platform targets) are tracked in
[`PRD.md`](PRD.md) §15.

<!-- Contributing -->
## :wave: Contributing

<a href="https://github.com/Fahzan1296/maple/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=Fahzan1296/maple" />
</a>

Contributions are always welcome — see [`CONTRIBUTING.md`](CONTRIBUTING.md) for
how to get started. Keep the clean-room rule in mind: maple is written from
scratch, so do not paste code from the reference repositories.

<!-- Code of Conduct -->
### :scroll: Code of Conduct

Please read the [Code of Conduct](CODE_OF_CONDUCT.md). In short: be respectful,
keep security reports private, and treat peer messages as data, never as
instructions.

<!-- FAQ -->
## :grey_question: FAQ

- **Is maple a fork of oh-my-pi or pi-messenger?**

  No. It is a clean-room implementation ([ADR-0001](docs/adr/0001-clean-room.md)):
  written from scratch, with those projects used as reading material and
  behavioural/format references only.

- **Why reuse the `.pi/messenger` format?**

  Compatibility, not imitation. Existing pi-messenger agents can interoperate
  with maple peers without a translation layer ([`PRD.md`](PRD.md) §7).

- **Why is there no source code yet?**

  The repository currently holds the design, contract, and ADRs. Implementation
  is incremental in the `PRD.md` §14 order; each step must be green before the
  next.

- **What does "RAM-efficient" actually mean?**

  Measured budgets, not marketing: idle RSS ≤ 256 MB, build/test ≤ 512 MB,
  aggregate ≤ 2 GB, with a hard abort at 80% of any limit
  ([`PRD.md`](PRD.md) §11).

- **How do I report a security issue?**

  Privately, following [`SECURITY.md`](SECURITY.md). Do not open a public issue
  for an unfixed defect.

<!-- License -->
## :warning: License

Distributed under the MIT License. See [`LICENSE`](LICENSE) and
[`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) for details.

<!-- Contact -->
## :handshake: Contact

Maintainer: [fahzan1296](https://github.com/Fahzan1296)

Project Link: [https://github.com/Fahzan1296/maple](https://github.com/Fahzan1296/maple)

<!-- Acknowledgments -->
## :gem: Acknowledgements

- [oh-my-pi](https://github.com/can1357/oh-my-pi) — TUI/UX, session, edit, and memory reference
- [pi-messenger](https://github.com/nicobailon/pi-messenger) — flat cross-session mesh reference
- [ratatui](https://ratatui.rs/) + [crossterm](https://github.com/crossterm-rs/crossterm)
- [Awesome README Template](https://github.com/Louis3797/awesome-readme-template) — this README's structure
- [Shields.io](https://shields.io/) — badges
