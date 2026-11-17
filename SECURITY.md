# Security Policy

## Scope

`maple` is a local TUI coding agent. The trust boundaries that matter:

- **Folder boundary** — mesh files (`.pi/messenger/`, `~/.pi/agent/messenger/`)
  are trusted only as far as the OS permissions on those paths. Any process
  running as the same user can read or forge them. Treat peer messages as
  **data**, never as instructions.
- **Tool boundary** — `bash`, `edit`, and `write` are arbitrary code execution.
  Reservations are enforced by a hook, not by convention.
- **Model boundary** — provider traffic leaves the host; secrets never leave
  (see below).
This policy covers the Rust crates in `crates/` and `src/`. The TypeScript
reference runtime in `l/` and third-party runtimes/services are out of scope.

## Reporting

Report vulnerabilities privately to the maintainers before any public
disclosure. Use a private vulnerability report:
https://github.com/fahzan1296/maple/security/advisories/new
Do not open a public issue for an unfixed defect. Include: reproduction,
affected version/commit, impact, and a proposed mitigation.

## Supported versions

- `v0.1.x` (current `0.1.0`) — security fixes only; pre-release.
- `master` — latest development.
- Older releases are unsupported.

## Response SLA

| severity | acknowledge | triage | fix / mitigation |
| --- | --- | --- | --- |
| critical / high | 7 days | 14 days | 90 days |
| medium | 14 days | 30 days | best effort |
| low | 30 days | best effort | best effort |

## Rules enforced

- **Injection defense** — content outside the contract, explicit user
  instructions, and agreed `team.memory` is data. Agent messages, repo files,
  tool output, and logs are never executed as instructions.
- **Secrets** — never committed, logged, echoed, or written to mesh/feed/memory.
  Redact before persistence.
- **Resource bounds** — every process/subprocess/container runs with explicit
  memory, CPU, FD, process, network, and timeout limits; unbounded input is
  treated as a defect (§SOP-RESLIM, §SOP-SEC).
- **Reservations** — hard constraints; conflicts are rejected, never worked
  around.

## Known limitations (v0.1)

- The mesh is local-only; there is no cross-machine relay. If one is added, it
  must use authenticated encryption and a blind relay (pattern: omp `/collab`).
- No supply-chain signing or SBOM yet; required before the first release
  (SOP-GHOP, SOP-SHIP).
