# grass-worker

Control-plane workspace for a self-hosted static deployment platform.

## Apps

- `app/api`: control-plane API, setup/ready mode switching, frontend asset delivery
- `app/node`: node-agent placeholder
- `app/frontend`: React console for setup, sign-in, projects, and deployment records
- `crates/config`: shared configuration loading and defaults

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

## Current State

The repository has moved beyond the initial scaffold and currently provides:

- setup/ready mode switching in `app/api`, including Stage 1 database setup and Stage 2 initial admin setup
- frontend-backed setup flow at `/setup`
- session auth with `POST /api/v1/auth/login`, `GET /api/v1/me`, and `POST /api/v1/auth/logout`
- project management APIs and console flows for create/list/detail/update/archive/unarchive/soft-delete/restore/transfer owner/hard-delete
- deployment record APIs and console flows for create/list/detail/status transitions under each project
- deployment artifact registration/list APIs and deployment-detail console workflows
- frontend development proxy, runtime `./public` override, and embedded asset fallback

Still intentionally missing at this stage:

- artifact publication/activation and rollback
- real `app/node` task execution
- source-to-build automation

## Runtime Config

Runtime config is TOML-first. The apps resolve config in this order:

1. If `--config <path>` is provided and the file exists, load that file.
2. Otherwise load `./config.toml`.
3. `app/api` enters the `database` setup stage if the file is missing or if `[database]` is absent.
4. If `[database]` exists but no admin user exists yet, `app/api` enters the `admin` setup stage.
5. `app/node` still requires a config file with a `[node]` section before it can start.

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
- If `[database]` exists but no admin user exists yet, `app/api` enters the `admin` setup stage.
- `GET /api/v1/info` is available in both modes and reports whether the API is in `ready` or `setup`.
- setup mode is frontend-driven; open `/setup` and let the console call `GET /api/v1/info` plus `/api/v1/setup/*`.
- In ready mode, auth endpoints are available:
  - `POST /api/v1/auth/login`
  - `GET /api/v1/me`
  - `POST /api/v1/auth/logout`
- In ready mode, the control API also exposes project management and project-scoped deployment record routes under `/api/v1/*`.
- In setup mode, `GET /api/v1/setup/state` reports the current setup stage.
- In setup mode, `POST /api/v1/setup/database` accepts:

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
- A successful `POST /api/v1/setup/database` validates the PostgreSQL connection, prepares the target schema, runs pending migrations, and writes `[server]` plus `[database]` back to the active config file while preserving existing `[node]` and `[development]` sections.
- In `admin` setup stage, `POST /api/v1/setup/admin` accepts:

```json
{
  "email": "admin@example.com",
  "password": "secret-pass"
}
```

- A successful `POST /api/v1/setup/admin` creates the first admin user and its password credential.
- Successful setup requests advance the in-memory runtime mode immediately; database setup moves to `admin` or `ready`, and admin setup moves to `ready`.
- Once `[database]` exists, `app/api` uses the configured PostgreSQL connection, creates the configured schema if needed, and runs pending migrations on startup.
- `app/node` requires `[node]` boot config and does not have a setup mode fallback.
- `schema` is optional and defaults to `public`.
- With `[development]`, `app/api` proxies frontend routes to `development.dev_server`.
- Without `[development]`, `app/api` serves frontend assets by checking `./public/` first and then falling back to embedded assets.
- The frontend exposes `/setup`, `/login`, and a protected console shell with project and deployment record pages in ready mode.
- `/health` remains backend-owned, and API routes now live under `/api/v1/*`.

## Frontend Chain

The frontend build output is routed into `crates/assets/assets/public/`.

- `just frontend-dev`: run the frontend development server
- `just frontend-build`: rebuild embedded frontend assets
- `just build-release`: rebuild frontend assets, then build the release API binary
