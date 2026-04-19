frontend-dev:
    cd app/frontend && bun run dev

frontend-build:
    cd app/frontend && bun run build

fmt:
    cargo fmt --all

check: frontend-build
    cargo check --workspace

test: frontend-build
    cargo test --workspace
    cd app/frontend && bun test

run-api:
    cargo run -p grass-worker-api

run-node:
    cargo run -p grass-worker-node

run-frontend: frontend-dev

run:
    bash -lc 'trap "kill 0" EXIT INT TERM; (cd app/frontend && bun run dev) & cargo run -p grass-worker-api & wait'

build-release: frontend-build
    cargo build --release -p grass-worker-api
