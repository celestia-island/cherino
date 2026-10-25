# cherino — container operations toolkit (workspace: cherino-macros / cherino /
# cherino-runtime).
#
# Verb-first dispatch: actions are first-level commands (build, test, check, …).

set shell := ["bash", "-c"]
set windows-shell := ["bash.exe", "-c"]
set unstable
set lists

# Shared celestia-devtools recipes — NOT in git. Stage with: just fetch.
# `import?` silently skips when absent, so this justfile parses pre-fetch.
import? "./.just/celestia-devtools.just"

default:
    @just --list

# ── Build ────────────────────────────────────────────────────────────

build:
    cargo build --workspace --all-features

check:
    cargo check --workspace --all-features

# ── Quality ──────────────────────────────────────────────────────────

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-features --tests -- -D warnings

lint: fmt-check clippy

test:
    cargo test --workspace

# Integration tests that require a live Docker daemon (opt-in).
test-docker:
    cargo test --workspace --features cherino/docker-tests

ci: check fmt-check clippy test
