# grass-worker

Initial scaffold for a self-hosted static deployment platform.

## Apps

- `app/api`: control-plane API placeholder
- `app/node`: node-agent placeholder
- `app/frontend`: frontend placeholder
- `crates/config`: shared configuration defaults

## Commands

```bash
just fmt
just check
just test
just run-api
just run-node
just run-frontend
```

## Scope

This repository currently contains only the initial runnable skeleton:

- Axum-based `api` and `node` services with `/` and `/health`
- Bun-powered frontend placeholder with `Hello, World`
- Minimal shared config defaults

The frontend currently uses a Vite-compatible placeholder under Bun so the desired frontend stack can be swapped in later without changing the repository shape.
