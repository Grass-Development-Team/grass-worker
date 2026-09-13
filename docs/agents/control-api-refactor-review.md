# Control API refactor review

Baseline: `20a57e48`. Branch: `refactor/control-api-conventions`. Tracking: [Control API conventions](https://github.com/Grass-Development-Team/grass-worker/issues/207), version 0.1.0.

## Result

- `features` follows the URL tree. The existing 220 method/path contracts have 177 endpoint owners. Dynamic segments use `by_<parameter>`, each endpoint file owns its local controllers and HTTP types, and parents compose immediate child routers.
- Request and response structs remain local to their endpoint. Deliberate open metadata/configuration retains `serde_json::Value`. Timestamp serialization, null fields and omitted fields retain their existing response contracts.
- `infra/audit` owns persistence, structured audit logs, redaction, request context, querying, pagination and retention. `AuditTransaction` emits committed-success logs after commit and discards pending logs on rollback. Request/external observations use explicit post-response or post-commit semantics.
- Shared deployment activation/cancellation/placement, project access/lifecycle, administration mutations, preview access, session issuance, MFA and host binding operations have named domain modules. DNS HTTP transport and provider clients remain infrastructure.
- Nullable PATCH fields use the shared parsing primitive with local request structs. Raw JSON null now clears the personal display name, consistent with administrator display-name updates and quota-plan detachment. Other fields retain their existing clearing contracts.
- Certificate, ownership, onboarding and ingress states use named types and explicit storage conversions; database enum and TEXT/CHECK representations are unchanged. Infrastructure errors retain sources, log redacted diagnostic chains and return safe responses. Region deletion reports genuine usage conflicts separately from infrastructure failures.
- Shared certificate, Node, user and DNS fixtures live under `src/test_support`; invitation scenarios live under `src/integration_tests`. Cross-endpoint regression tests issue HTTP requests through route modules. Tests requiring disposable PostgreSQL schemas are explicitly ignored by default.
- Control API dependencies inherit workspace versions, including the X.509 parser used by Node. Application-specific dependency features remain additive.

## Review entry points

- `docs/agents/control-api-conventions.md`: lasting conventions and validation boundaries.
- `docs/agents/control-api-routes.md`: original method/path inventory and final module ownership.
- `apps/control-api/src/features/api/v1.rs`: authentication/mounting boundaries and route composition.
- `apps/control-api/src/infra/audit`: transaction, log, redaction and query contracts.
- `apps/control-api/src/features/api/v1/projects/by_project_id/deployments`: deployment URL slices.
- `apps/control-api/src/features/api/v1/internal`: authenticated Build/Serve/Node protocol slices.
- `apps/control-api/src/domain/preview_access.rs`: user preview authorization and screenshot grants.
- `apps/control-api/src/features/api/v1/me.rs`: raw JSON and HTTP display-name clearing regressions.
- `apps/control-api/src/features/api/v1/admin/regions/by_code.rs`: conflict versus infrastructure response regression.

## Validation boundary

Each implementation commit is preceded by `just quality`, covering formatting, workspace Clippy, Rust tests, Console tests/checks, workspace checks/builds and license checks. Targeted checks additionally cover route ownership, Node protocol wire fixtures, response privacy/nullability, audit commit/rollback/log behavior, certificate validation and mock ACME issuance.

The final full validation passed: Control API 523 passed / 32 ignored; Node 134 passed / 2 ignored; Console 239 passed across 63 files. The route ownership check found 177 endpoint owners with no violations. These counts describe the executed checks, with ignored cases reported separately.

No dedicated PostgreSQL or Redis test environment was configured for this work. Ignored database/schema/Redis tests are compiled but are not executed database regressions. Node tests requiring external tools or services may also be ignored. No database schema migration or disposable database operation is part of this refactor validation.

The R1 TODO block remains until all implementation is validated and merged. The last task then removes the entire block, including R1.9, while preserving other unfinished TODO scope. Implementation review precedes PR creation, and merge requires separate explicit approval under `AGENTS.md`.
