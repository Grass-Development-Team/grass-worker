set shell := ["sh", "-c"]
set windows-shell := ["pwsh.exe", "-NoProfile", "-Command"]
set dotenv-load := false

_default:
    just --list

fmt:
    cargo fmt --all
    bun run --cwd apps/console --bun fmt

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace
    bun run --cwd apps/console --bun test

check:
    cargo check --workspace
    bun run --cwd apps/console --bun check

quality: fmt clippy test check build

run target:
    {{ if target == "api" { "cargo run -p grass-control-api" } else if target == "node" { "cargo run -p grass-node" } else if target == "console" { "bun run --cwd apps/console --bun dev" } else { error("unknown run target: " + target) } }}

build target="all":
    {{ if target == "api" { "cargo build -p grass-control-api" } else if target == "node" { "cargo build -p grass-node" } else if target == "console" { "bun run --cwd apps/console --bun build" } else if target == "all" { "cargo build --workspace && bun run --cwd apps/console --bun build" } else { error("unknown build target: " + target) } }}
