# maple — task runner. Run `just` for the list.
set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

# ---- build & test -----------------------------------------------------------

build:
    cargo build --workspace

check:
    cargo check --workspace --all-targets

# Unit tests of every crate. Integration binaries are separate (`just test-int`).
test *args:
    cargo nextest run --workspace --lib {{args}}

# Integration tests of the TUI toolkit (330 binaries — bounded, see nextest.toml).
test-int *args:
    cargo nextest run -p maple-tui --tests {{args}}

# ---- quality gates ---------------------------------------------------------

fmt:
    cargo fmt --all

# Advisory until the formatting pass lands; see rustfmt.toml.
fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

deny:
    cargo deny check

typos:
    typos

toml:
    taplo fmt --check --diff && taplo lint

actions:
    actionlint

# Everything CI enforces, in one run.
ci: clippy deny typos toml actions test

docs:
    cargo doc --workspace --no-deps --document-private-items

# ---- setup -----------------------------------------------------------------

# Install the extra tooling the recipes above expect (prebuilt binaries).
tools:
    bash scripts/install-tools.sh
