# Control API conventions

## Route slices

`features` follows the actual URL tree. Each file owns one path and may contain controllers for several HTTP methods on that same path. It owns its request, query, path, response, validation, and endpoint orchestration types and functions. A path with no endpoint only composes child routers.

A module composes its own endpoint and its immediate children. Parent routers call child routers, not descendant controllers. Map `{project_id}` to `by_project_id.rs`; map hyphenated URL segments to snake_case Rust names. Preserve exact URL and trailing-slash behavior when composing Axum routers.

For example, `features/api/v1/projects.rs` owns GET and POST `/api/v1/projects`; `projects/by_project_id.rs` owns GET and PATCH `/api/v1/projects/{project_id}`; `projects/by_project_id/deployments/by_deployment_id/cancel.rs` owns POST `/api/v1/projects/{project_id}/deployments/{deployment_id}/cancel`.

Controllers and router entrypoints use the narrowest required visibility. A sibling endpoint or background job must not use a controller module as a utility library.

## Local contracts

Every endpoint owns its HTTP request and response types. Different endpoints may duplicate structs even when their fields currently match. Do not extract business DTOs merely to avoid duplication. Shared envelopes, timestamp serialization, protocol primitives, and genuine business capabilities may be reused.

Construct complex responses as typed values after resolving their data. Keep `serde_json::Value` for deliberately open metadata and configuration, not for incrementally patching a stable response shape. Preserve field names, nullability, defaults, status codes, and wire values. Validate Node-facing local contracts against `grass-node-protocol`.

PATCH fields that support clearing must distinguish missing, explicit null, and a supplied value. Use an endpoint-local `Option<Option<T>>` with `#[serde(default, deserialize_with = "crate::infra::http::patch::nullable")]`. Missing leaves the value unchanged; null clears it. Keep fields that do not accept clearing on their existing contract. Test raw JSON at the deserialization boundary and the resulting update.

## Business capabilities and infrastructure

A route slice may use SeaORM directly and may orchestrate its use case. Shared project lifecycle, deployment cancellation, quota enforcement, host binding, and preview authorization belong to named business capabilities under `domain`. Provider clients, DNS transport, storage backends, and other technical adapters belong to `infra`.

Prefer direct functions and concrete types. Do not add repository traits, generic service layers, or shared DTO catalogs without an actual need. Keep endpoint-specific authorization explicit. A helper that loads data must not conceal a read/write permission choice behind an unexplained boolean.

Single-path files such as administration settings may remain substantial. Organize their local validation, preparation, persistence, and response construction by responsibility; do not invent extra URL paths to shorten a file.

## Audit

All audit event persistence and related structured logging go through `infra/audit`. That module owns event/context types, redaction, database writing, log output, HTTP request middleware, queries, and retention. Endpoint request/response DTOs remain local.

Business code supplies the actor, operation, target, result, changes, metadata, and explicit visibility. Request IDs correlate request audit and business events. Preserve platform/team visibility and existing request sampling/exclusion rules.

Critical database mutations and their business audit must share a transaction. Audit insertion failure rolls back the business change. Emit committed-success audit logs only after the transaction commits; a rolled-back insertion is not a committed business success. Request audit runs after the response is produced: failures emit a structured error and do not replace the completed business response. External actions and post-commit cleanup record actual outcomes and expose failures explicitly.

The audit table is the durable source of truth. Database commit and external log output are not atomic. Do not promise exactly-once log delivery. Redact event metadata and changes before either sink receives them, and never log credentials, session cookies, bearer tokens, or private keys.

## Errors, state, and readability

Keep business rejections distinct from infrastructure failures. Preserve safe diagnostic context; do not discard sources or convert arbitrary database errors to Conflict. Use named business state types and explicit `parse` / `as_str` storage conversions without changing database enum representation merely for style. Certificate, domain-check, ownership, and ingress-health states retain their existing TEXT/CHECK storage values; unknown values must not silently become a valid state.

Place local tests after production items. Shared fixtures belong in test-only support modules; cross-endpoint tests should exercise HTTP boundaries. Do not expose one feature's test module as another feature's fixture API. Shared certificate, Node, user and DNS fixtures live under `src/test_support`; invitation HTTP scenarios live under `src/integration_tests`. Large domain test suites use test-only child modules.

Manually expand dense SQL, JSON, and mock closures where rustfmt cannot express the logical steps. Keep import grouping and blank lines consistent. Prefer semantic names over boolean switches and generic helpers with hidden side effects.

## Validation and tracking

Protect route paths/methods, authentication middleware, response contracts, lifecycle transactions, audit isolation/redaction, and preview grants with relevant tests. PostgreSQL suites that create disposable schemas are explicitly ignored in ordinary runs. Running them requires `GRASS_TEST_DATABASE_URL` and authorization for disposable schema creation and cleanup; Redis cases additionally require `GRASS_TEST_REDIS_URL`. They fail when explicitly selected without that configuration. CI runs the complete PostgreSQL and Redis suite through `just test-services` against disposable services, including delivery, Node deletion, migration shape/rollback, authentication revocation and cache regressions. The Chromium screenshot case is excluded from this service suite. Do not count ignored or unconfigured database cases as executed regressions.

Use the repository Just commands and Vite+ for Console checks. Each commit requires a successful `just quality`. Follow the issue, worktree, review, PR, merge, and TODO cleanup gates in `AGENTS.md`.
