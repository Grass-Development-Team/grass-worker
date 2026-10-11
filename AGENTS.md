# Agent Guidelines

## Required Reading

At the start of each task, read [CONTRIBUTING.md](CONTRIBUTING.md). It defines setup,
commands, code and file conventions, commits, PRs, and licensing for humans and agents.
Use [docs/todo.md](docs/todo.md) for the current version and remaining scope; Future
items are not current functionality. Follow any more specific directory instructions.

## Repository Layout

- `apps/control-api`: Axum API, domain capabilities, infrastructure, and SeaORM migrations.
- `apps/node`: container builds, artifacts, static/SSR delivery, and Node lifecycle.
- `apps/console`: Lit Web Components, TypeScript, and Vite 8.
- `crates`: shared Rust libraries and Node protocol contracts.
- `docs`: configuration, operation, architecture, and development references.
- `scripts`: asset, service-test, and release-policy validation.

## Toolchain

- Rust 2024, MSRV 1.88; use Cargo and the root Justfile.
- Console uses Bun and Vite 8. Prefer root Just recipes (e.g. `just check console`),
  or Bun scripts in `apps/console/package.json`.
- Bun manages `apps/console/bun.lock`; preserve both committed dependency lockfiles.
- `just quality` covers local formatting, linting, tests, checks, and builds.
  `just test-services`, `just msrv`, and `just audit` are separate checks.
- Release builds require the Console output first; use `just release`.

## Change Constraints

- Control API feature files own one URL path and local endpoint DTOs. Shared business
  capabilities belong in `domain`, technical adapters in `infra`, and auditing in
  `infra/audit`. See CONTRIBUTING.md for the detailed contracts.
- For Console changes, read [CODE_STYLE.md](apps/console/CODE_STYLE.md) and
  [DESIGN.md](apps/console/DESIGN.md). Keep business requests in service modules.
- Preserve API contracts, authorization, tenant isolation, Host binding, preview
  grants, and path traversal protections for artifacts, logs, static files, and archives.
- Schema changes require direct inspection of migration state, columns, enums,
  defaults, indexes, and foreign keys. Use disposable test services; never reset
  shared infrastructure for validation.
- Keep secrets and local configuration out of source control and logs. Frontend
  `VITE_*` values are public. Do not commit generated assets or test artifacts.
- Keep changes focused and preserve unrelated user edits. Update relevant regression
  coverage and distinguish executed tests from skipped or unavailable checks.

## Workflow Conventions

1. After any code change, review all repository documentation for affected behavior,
   commands, configuration, APIs, and examples, and update every affected document.
   Code and documentation must remain consistent.
