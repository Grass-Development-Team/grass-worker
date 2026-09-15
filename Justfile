root := justfile_directory()
console := root / "apps" / "console"

[unix]
set shell := ["sh", "-c"]

[windows]
set shell := ["pwsh.exe", "-NoProfile", "-Command"]
set dotenv-load := false

_default:
    just --list

fmt target="all":
    {{ if target == "rust" { "cargo fmt --all" } else if target == "console" { "cd " + console + " && vp check --fix" } else if target == "all" { "cargo fmt --all && cd " + console + " && vp check --fix" } else { error("unknown fmt target: " + target) } }}

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test target="all":
    {{ if target == "rust" { "cargo test --workspace" } else if target == "console" { "cd " + console + " && vp test" } else if target == "all" { "cargo test --workspace && cd " + console + " && vp test" } else { error("unknown test target: " + target) } }}

check target="all":
    {{ if target == "rust" { "cargo check --workspace" } else if target == "console" { "cd " + console + " && vp check" } else if target == "all" { "cargo check --workspace && cd " + console + " && vp check" } else { error("unknown check target: " + target) } }}

quality: fmt clippy test check build assets-check release-check license-check

license-check:
    test -f LICENSE
    grep -q 'license = "BSD-3-Clause"' Cargo.toml
    grep -q '"license": "BSD-3-Clause"' apps/console/package.json

audit:
    cargo audit
    cd {{ console }} && vp pm audit

run target:
    {{ if target == "api" { "cargo run -p grass-control-api" } else if target == "node" { "cargo run -p grass-node" } else if target == "console" { "cd " + console + " && vp dev" } else { error("unknown run target: " + target) } }}

build target="all":
    {{ if target == "api" { "cd " + console + " && vp build && cargo build -p grass-control-api" } else if target == "node" { "cargo build -p grass-node" } else if target == "console" { "cd " + console + " && vp build" } else if target == "rust" { "cd " + console + " && vp build && cargo build --workspace" } else if target == "all" { "cd " + console + " && vp build && cargo build --workspace" } else { error("unknown build target: " + target) } }}

install target="all":
    {{ if target == "console" { "cd " + console + " && vp install" } else if target == "all" { "cd " + console + " && vp install" } else { error("unknown install target: " + target) } }}

preview target="console":
    {{ if target == "console" { "cd " + console + " && vp preview" } else { error("unknown preview target: " + target) } }}

migrate:
    cargo run -p grass-control-api -- migrate

# Verify the locked workspace with its declared minimum supported Rust version.
msrv:
    cargo msrv verify --manifest-path apps/control-api/Cargo.toml --no-log -- cargo check --workspace --all-targets --locked

# Build distributable binaries with the production Console embedded.
release:
    cd {{ console }} && vp build
    cargo build --release --locked -p grass-control-api -p grass-node

assets-check:
    python3 scripts/check-embedded-assets.py

# This suite creates and removes test schemas in the configured disposable services.
test-services:
    python3 scripts/test-services.py

release-check:
    python3 -m unittest discover -s scripts -p "test_release_metadata.py"
