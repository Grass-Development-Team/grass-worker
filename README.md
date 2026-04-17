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
just frontend-build
just frontend-dev
just run-api
just run-node
just run-frontend
just build-release
```

## Scope

This repository currently contains only the initial runnable skeleton:

- Axum-based `api` and `node` services with `/` and `/health`
- Bun-powered frontend placeholder with `Hello, World`
- Minimal shared config defaults

The frontend currently uses a Vite-compatible placeholder under Bun so the desired frontend stack can be swapped in later without changing the repository shape.

## Runtime Config

Runtime config is TOML-first. The apps resolve config in this order:

1. If `--config <path>` is provided and the file exists, load that file.
2. Otherwise load `./config.toml`.
3. `app/api` enters setup mode if the file is missing or if `[database]` is absent.
4. `app/node` still requires a config file with a `[node]` section before it can start.

Example:

```toml
[server]
listen = "127.0.0.1:3000"

[node]
listen = "127.0.0.1:3001"

# [database]
# host = "127.0.0.1"
# port = 5432
# db_name = "grass_worker"
# user = "postgres"
# password = "postgres"
# schema = "public"

[development]
dev_server = "http://127.0.0.1:5173"
```

Rules:

- `app/api` enters setup mode on `server.listen` when `config.toml` or `[database]` is missing.
- `GET /api/info` is available in both modes and reports whether the API is in `ready` or `setup`.
- Setup mode is API-only; the backend no longer serves placeholder HTML on `/`.
- In setup mode, `GET /api/setup/state` reports the current setup stage.
- In setup mode, `POST /api/setup/database` accepts:

```json
{
  "host": "127.0.0.1",
  "port": 5432,
  "db_name": "grass_worker",
  "user": "postgres",
  "password": "postgres",
  "schema": "public"
}
```

- `schema` is optional; missing or blank values default to `public`.
- A successful `POST /api/setup/database` validates the PostgreSQL connection, prepares the target schema, runs pending migrations, and writes `[server]` plus `[database]` back to the active config file while preserving existing `[node]` and `[development]` sections.
- Once `[database]` exists, `app/api` uses the configured PostgreSQL connection, creates the configured schema if needed, and runs pending migrations on startup.
- `app/node` requires `[node]` boot config and does not have a setup mode fallback.
- `schema` is optional and defaults to `public`.
- With `[development]`, `app/api` proxies frontend routes to `development.dev_server`.
- Without `[development]`, `app/api` serves frontend assets by checking `./public/` first and then falling back to embedded assets.
- `/health` and `/api/*` remain backend-owned in both modes.

## Frontend Chain

The frontend build output is routed into `crates/assets/assets/public/`.

- `just frontend-dev`: run the frontend development server
- `just frontend-build`: rebuild embedded frontend assets
- `just build-release`: rebuild frontend assets, then build the release API binary
