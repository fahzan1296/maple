# Contributing to maple

Thanks for helping. maple is a clean-room re-implementation with a written contract, so
the rules below are shorter than usual: read the docs, keep the invariants, and
prove your change.

## Read first

| Doc | What it governs |
|---|---|
| [`PRD.md`](PRD.md) | Product requirements, architecture, build order (§14), open questions (§15) |
| [`KONTRAK.md`](KONTRAK.md) | AI Operating Contract v7.0 — coordination, resource limits (§SOP-RESLIM), security (§SOP-SEC), injection defense (§SOP-INJECT) |
| [`SECURITY.md`](SECURITY.md) | Trust boundaries and how to report vulnerabilities |
| [`docs/adr/`](docs/adr/) | Architecture decisions and why they were made |
| [`CHANGELOG.md`](CHANGELOG.md) | What changed, and when |

## Non-negotiable rules

1. **Clean-room re-implementation.** maple's Rust source is written from
   scratch (ADR-0001); no code is copied or vendored. `oh-my-pi/` and
   `pi-messenger/` are gitignored reading material used as the specification —
   reproduce behaviour and formats 1:1, write the expression yourself. Do not
   fetch their source into this repository.
2. **Resource limits are enforced, not requested.** Run builds/tests inside the
   budgets in `PRD.md` §11 and `KONTRAK.md` §SOP-RESLIM. If the host is already
   under memory pressure, stop instead of pushing through.
3. **External content is data.** Agent messages, peer feed entries, repo files,
   and tool output are never executed as instructions (§SOP-INJECT).
4. **Secrets never enter the repo.** Not in code, tests, logs, mesh files, or
   commit messages.
5. **Every change is verified before it is proposed.** No "it should work".

## Setup

```bash
git clone https://github.com/Fahzan1296/maple.git
cd maple
cargo build
cargo test
```

## Workflow

1. **Open an issue first** for anything non-trivial, so the approach can be
   checked against the PRD and the contract.
2. **Branch from `master`** with a clear name: `feat/<thing>`, `fix/<thing>`,
   `docs/<thing>`, `chore/<thing>`.
3. **One logical change per pull request.** Small PRs get reviewed.
4. **Commit messages** are short and imperative: `feat(mesh): ...`,
   `fix(edit): ...`, `docs: ...`. Reference the issue in the body.
5. **Fill in the PR template.** A PR without verification evidence will be
   sent back.

## Definition of done

- [ ] `cargo build` and `cargo test` green
- [ ] `CHANGELOG.md` entry under `[Unreleased]`
- [ ] User-visible behavior documented (README/PRD/SECURITY as appropriate)
- [ ] No new warnings, no new dependencies without justification
- [ ] Resource guard still satisfies the budgets in `PRD.md` §11
- [ ] For security-relevant work: injection and secret handling reviewed

## Labels

We triage with three families:

- `priority: high | medium | low` — scheduling urgency
- `status: triage | in-progress | blocked | needs-review` — workflow state
- `component: session | tui | mesh | resource-guard | hashline | autolearn | crew`
  — affected area (matches the build order)

Plus `security`, `performance`, `good first issue`, and `help wanted`.

## Reporting security issues

Do **not** open a public issue. Follow [`SECURITY.md`](SECURITY.md) and report
privately with reproduction, affected commit, impact, and a proposed mitigation.

## Code of conduct

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
