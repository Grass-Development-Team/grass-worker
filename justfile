fmt:
    cargo fmt --all

check:
    cargo check --workspace

test:
    cargo test --workspace
    cd app/frontend && bun test

run-api:
    cargo run -p grass-api

run-node:
    cargo run -p grass-node

run-frontend:
    cd app/frontend && bun run dev
