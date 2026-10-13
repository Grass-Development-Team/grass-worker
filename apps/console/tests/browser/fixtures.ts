import { expect, type Page } from "@playwright/test";

export const user = {
  id: "u1",
  email: "person@example.com",
  display_name: "Alex",
  avatar_url: null,
  platform_role: "admin",
  email_verified: true,
};
export const team = {
  id: "t1",
  slug: "acme",
  name: "Acme",
  kind: "team",
  avatar_url: null,
  owner_user_id: "u1",
  group_id: "g1",
  role: "owner",
};
export const project = {
  id: "p1",
  team_id: "t1",
  slug: "website",
  name: "Website",
  runtime: "static",
  repository_url: "https://github.com/acme/website.git",
  default_branch: "main",
  install_command: "bun install",
  build_command: "bun run build",
  output_directory: "dist",
  source_config: { root_directory: ".", framework_hint: "vite" },
  build_config: {},
  archived_at: null,
  deleted_at: null,
  created_at: "2026-10-14T00:00:00Z",
  updated_at: "2026-10-14T00:00:00Z",
};
export const deployment = {
  id: "d1",
  project_id: "p1",
  team_id: "t1",
  region: "local",
  build_node: { id: "n1", name: "Node" },
  serve_node: { id: "n1", name: "Node" },
  environment: "production",
  runtime_kind: "static",
  build_status: "ready",
  serve_status: "ready",
  release_status: "active",
  release_pending: false,
  pending_release_reason: null,
  pending_release_requested_at: null,
  serve_resources: { cpu_millicores: 100, memory_mb: 128, disk_mb: 100 },
  overcommitted: false,
  build_stage: "complete",
  source: {
    repository_url: project.repository_url,
    branch: "main",
    commit_hash: "abcdef123456",
    commit_message: "Ship the website",
  },
  triggered_by: user,
  failure_code: null,
  failure_message: null,
  serve_failure_code: null,
  serve_failure_message: null,
  duration_seconds: 12,
  created_at: project.created_at,
  preview_url: null,
  production_url: null,
  screenshot_status: "unavailable",
  screenshot_url: null,
};
const policy = {
  min_length: 8,
  max_length: 1024,
  require_lowercase: true,
  require_uppercase: false,
  require_number: false,
  require_symbol: false,
  history_count: 3,
};
const mfaPolicy = {
  allowed_factors: ["totp", "email"],
  enforcement: "none",
  minimum_factors: 0,
  required_factors: [],
};
const factor = {
  id: "f1",
  kind: "totp",
  label: null,
  verified: true,
  created_at: project.created_at,
  last_used_at: null,
};
const plan = {
  id: "q1",
  code: "default",
  name: "Default",
  description: null,
  is_default: true,
  enabled: true,
  limits: [],
  source: "default",
};
const group = {
  id: "g1",
  code: "default",
  name: "Default",
  description: null,
  quota_plan_id: "q1",
  review_policy: { production: null, preview: null, domain: null },
  is_default: true,
  team_count: 1,
  created_at: project.created_at,
};
const storage = {
  backend: "local",
  local_root: "/data",
  endpoint: "",
  region: "us-east-1",
  bucket: "",
  prefix: "",
  force_path_style: false,
  allow_http: false,
  credentials_configured: false,
};
export const settings = {
  site: {
    name: "Grass Worker",
    logo_url: null,
    url: "https://console.example.com",
    public_base_url: "https://sites.example.com",
  },
  signup: { policy: "open" },
  review: { production: "auto", preview: "auto" },
  domain_review: { default: "auto" },
  server: { host: "127.0.0.1", port: 7817 },
  database: { url_configured: true },
  redis: { backend: "moka", url_configured: false },
  secrets: { secret_key_configured: true, git_credentials_configured: true },
  mail: {
    mode: "none",
    from_address: "noreply@example.com",
    from_name: "Grass Worker",
    sendmail_command: "sendmail",
    smtp_host: "",
    smtp_port: 587,
    smtp_security: "starttls",
    smtp_username: "",
    smtp_password_configured: false,
  },
  authentication: {
    password_policy: policy,
    registration_email_verification: false,
    mfa_policy: mfaPolicy,
  },
  session: { cookie_secure: true, idle_ttl_seconds: 3600, session_ttl_seconds: 86400 },
  audit: { retention_days: 30 },
  node_manager: {
    auto_start_local_node: false,
    local_node_binary: "grass-node",
    local_node_config: "/data/node.toml",
    restart_on_exit: true,
  },
  migration: { auto_migrate: true },
  log: { level: "info", format: "pretty" },
  restart_required_sections: [],
};

export const accountSecurity = {
  email_verified: true,
  factors: [],
  allowed_factors: ["totp", "email"],
  mfa_required: false,
  mfa_requirements: { minimum_factors: 0, required_factors: [] },
  mfa_policy: { inherit_platform: true, minimum_factors: 0, required_factors: [] },
  password_policy: policy,
  mail_available: true,
};
const capacity = { cpu_millicores: 1000, memory_mb: 1024, disk_mb: 2048, max_deployments: 10 };
const configuration = {
  node: {
    id: "n1",
    region: "local",
    control_api: "https://console.example.com",
    work_root: "/data/node",
    capabilities: { build: true, serve: true },
  },
  build: { concurrency: 1, command_timeout_seconds: 600, retain_workspace_on_failure: false },
  serve: {
    host: "127.0.0.1",
    port: 7818,
    public_base_url: "https://sites.example.com",
    metadata_cache_ttl_seconds: 30,
    artifact_cache_root: "/data/cache",
    capacity,
    ssr: { idle_stop_seconds: 300, startup_timeout_seconds: 30 },
  },
  runtime: {
    backend: "docker-socket",
    socket: "/var/run/docker.sock",
    default_build_image: "grass-build",
    default_serve_image: "grass-serve",
    network: "bridge",
    resources: { cpu_limit: 1, memory_mb: 1024 },
  },
  security: { gateway_authentication: "token", private_repository_targets: [] },
  development: { verbose_build_log: false },
  log: { level: "info", format: "pretty" },
};
const node = {
  id: "n1",
  name: "Node",
  region: "local",
  status: "active",
  healthy: true,
  build_enabled: true,
  serve_enabled: true,
  build_concurrency: 1,
  base_url: null,
  work_root: "/data/node",
  version: "0.1.0",
  capacity,
  usage: { cpu_millicores: 0, memory_mb: 0, disk_mb: 0, deployments: 0 },
  overflow_count: 0,
  deletion: null,
  configuration: {
    desired: configuration,
    effective: configuration,
    desired_revision: 1,
    effective_revision: 1,
    status: "applied",
    error: null,
    node_token_configured: true,
  },
  last_heartbeat_at: project.created_at,
  created_at: project.created_at,
};
const localProcess = {
  auto_start: false,
  managed: true,
  process: {
    state: "stopped",
    pid: null,
    started_at: null,
    restart_count: 0,
    last_exit_code: null,
    last_exit_at: null,
    message: null,
  },
};
const host = {
  id: "h1",
  project_id: "p1",
  host: "app.example.com",
  region: "local",
  kind: "custom",
  environment: "production",
  status: "active",
  failure_reason: null,
  is_primary: true,
  host_source_id: null,
  serving: true,
  created_at: project.created_at,
  ownership_status: "verified",
  connection_state: "ready",
  certificate: {
    status: "active",
    issuer: "letsencrypt",
    platform_issuer: "letsencrypt",
    https_ready: true,
    installed_nodes: 1,
    required_nodes: 1,
    challenge_method: "http01",
    auto_renew: true,
    issued_at: project.created_at,
    expires_at: "2027-01-01T00:00:00Z",
    error: null,
    retry_at: null,
    revision: "1",
  },
  ingress: {
    region: "local",
    cname: { record_type: "CNAME", name: "app", target: "entry.example.com" },
    txt: { record_type: "TXT", name: "_grass.app", value: "fixture-proof" },
  },
};
const pagination = { page: 1, per_page: 25, total: 1, total_pages: 1 };
const adminTeam = {
  ...team,
  group: { id: "g1", name: "Default", code: "default" },
  explicit_quota_plan_id: null,
  member_count: 1,
  created_at: project.created_at,
};
const member = {
  id: "m1",
  user_id: "u1",
  email: user.email,
  display_name: user.display_name,
  role: "owner",
  joined_at: project.created_at,
};
const credential = {
  id: "c1",
  team_id: "t1",
  name: "GitHub token",
  kind: "https",
  host: "github.com",
  port: 443,
  username: "git",
  current_version_id: "v1",
  revoked_at: null,
  created_at: project.created_at,
  updated_at: project.created_at,
};
const announcement = {
  id: "a1",
  title: "Welcome",
  content: "Welcome to your workspace.",
  auto_popup: false,
  published_at: project.created_at,
};
export interface FixtureOptions {
  loggedIn?: boolean;
  role?: string;
  teamRole?: string;
  mfa?: boolean;
  mfaEmail?: boolean;
  setup?: boolean;
}
export async function installApi(page: Page, options: FixtureOptions = {}) {
  const calls: { path: string; method: string; body: unknown; headers: Record<string, string> }[] =
    [];
  let loggedIn = options.loggedIn ?? true;
  let profile = { ...user, platform_role: options.role ?? "admin" };
  const challenge = {
    mfa_required: true,
    mfa_enrollment_required: false,
    challenge_token: "fixture-challenge",
    factors: [factor, ...(options.mfaEmail ? [{ ...factor, id: "f2", kind: "email" }] : [])],
    allowed_factors: ["totp", "email"],
    return_to: "/projects",
  };
  await page.route("**/health", (route) =>
    route.fulfill({
      json: {
        status: "ok",
        service: "grass-control-api",
        version: "0.1.0",
        setup: options.setup ?? false,
      },
    }),
  );
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname,
      method = request.method();
    let body: unknown;
    try {
      body = request.postDataJSON();
    } catch {
      body = request.postData();
    }
    calls.push({ path, method, body, headers: request.headers() });
    const ok = (data: unknown, status = 200) =>
      route.fulfill({ status, json: { code: status, message: "OK", data } });
    if (path === "/api/v1/site-config")
      return ok({ site_name: "Grass Worker", logo_url: null, version: "0.1.0" });
    if (path === "/api/v1/auth/providers")
      return ok({
        password_recovery_available: true,
        registration_email_verification: false,
        signup_policy: "open",
        password_policy: policy,
      });
    if (path === "/api/v1/auth/csrf") return ok({ csrf_token: "fixture-csrf" });
    if (path === "/api/v1/auth/login" || path === "/api/v1/auth/mfa/verify") {
      if (options.mfa && path.endsWith("login")) return ok(challenge);
      loggedIn = true;
      return ok({ user: profile, csrf_token: "fixture-csrf" });
    }
    if (path === "/api/v1/auth/mfa/challenge") return ok(challenge);
    if (path === "/api/v1/auth/logout") {
      loggedIn = false;
      return ok({ message: "Signed out" });
    }
    if (path === "/api/v1/me") {
      if (!loggedIn)
        return route.fulfill({
          status: 401,
          json: { code: 401, message: "Authentication required", data: null },
        });
      if (method === "PATCH") profile = { ...profile, ...(body as object) };
      return ok({ user: profile });
    }
    if (path === "/api/v1/me/security")
      return ok({
        email_verified: true,
        factors: [factor],
        allowed_factors: ["totp", "email"],
        mfa_required: false,
        mfa_requirements: { minimum_factors: 0, required_factors: [] },
        mfa_policy: { inherit_platform: true, minimum_factors: 0, required_factors: [] },
        password_policy: policy,
        mail_available: true,
      });
    if (path.endsWith("/mfa/totp/start"))
      return ok({
        factor,
        secret: "FIXTURESETUPKEY",
        otpauth_uri: "otpauth://totp/Fixture?secret=JBSWY3DPEHPK3PXP&issuer=Fixture",
      });
    if (path.endsWith("/mfa/email/start") || path.endsWith("/mfa/email/send"))
      return ok({ factor: { ...factor, kind: "email" } });
    if (path === "/api/v1/setup/state") return ok({ stage: "database" });
    if (path === "/api/v1/teams") return ok({ teams: [team] });
    if (path === "/api/v1/teams/t1")
      return ok({ team: { ...team, role: options.teamRole ?? "owner" } });
    if (path.endsWith("/invitation-candidates"))
      return ok({
        candidates: [
          { kind: "email", user_id: null, email: "member@example.com", display_name: null },
        ],
      });
    if (path.endsWith("/members")) return ok({ members: [member] });
    if (path.endsWith("/invitations"))
      return ok({
        invitation: {
          id: "i1",
          team_id: "t1",
          email: "member@example.com",
          role: "member",
          status: "pending",
          expires_at: "2027-01-01",
          token: "fixture-invitation",
        },
      });
    if (path.endsWith("/source-credentials")) return ok({ credentials: [credential] });
    if (path.endsWith("/ssh-host-keys")) return ok({ host_keys: [] });
    if (path.endsWith("/quota") || path.endsWith("/quota/usage"))
      return ok({
        plan,
        limits: [],
        usage: [{ dimension: "projects", used: 1, limit: 10, period: "none" }],
      });
    if (path.endsWith("/audit-events") && !path.includes("/cleanup/"))
      return ok({ events: [], pagination });
    if (path === "/api/v1/projects")
      return ok(
        method === "POST"
          ? { project, host_assignment: { assigned: true } }
          : { projects: [project] },
      );
    if (path === "/api/v1/projects/p1")
      return ok({ project, team, role: options.teamRole ?? "owner" });
    if (path === "/api/v1/projects/p1/deployments") return ok({ deployments: [deployment] });
    if (path === "/api/v1/projects/p1/deployments/d1")
      return ok({
        deployment,
        events: [],
        artifacts: [],
        reviews: [],
        review_required: false,
        was_active: true,
      });
    if (path.endsWith("/build-log"))
      return ok({
        lines: [{ seq: 1, stage: "build", line: "Build completed", timestamp_ms: 1 }],
        last_seq: 1,
        build_status: "ready",
      });
    if (path.endsWith("/serve-nodes"))
      return ok({
        serve_nodes: [
          {
            ...node,
            normal_available: true,
            schedulable: true,
            overflow_only: false,
            disk_available_mb: 2048,
            remaining_overflow_slots: 1,
          },
        ],
      });
    if (path.endsWith("/hosts")) return ok({ hosts: [host] });
    if (path.endsWith("/source-credential")) return ok({ credential: null });
    if (path === "/api/v1/regions")
      return ok({ regions: [{ code: "local", name: "Local", created_at: project.created_at }] });
    if (path === "/api/v1/announcements") return ok({ announcements: [announcement], pagination });
    if (path === "/api/v1/notifications") return ok({ notifications: [], pagination });
    if (path.endsWith("/unread-count")) return ok({ count: 0 });
    if (path.endsWith("/auto-popup")) return ok({ notification: null });
    if (path === "/api/v1/team-invitations/preflight")
      return ok({
        team,
        role: "member",
        status: "pending",
        expires_at: "2027-01-01T00:00:00Z",
        email_matches_current_user: true,
        can_accept: true,
      });
    if (path === "/api/v1/admin/settings") return ok(settings);
    if (path === "/api/v1/admin/nodes") return ok({ nodes: [node], local_process: localProcess });
    if (path.endsWith("/deletion-plan"))
      return ok({
        node_id: "n1",
        assigned_deployments: 0,
        active_builds: 0,
        requires_target: false,
        eligible_targets: [],
      });
    if (path === "/api/v1/admin/users")
      return ok({
        users: [
          { ...profile, status: "active", last_login_at: null, created_at: project.created_at },
        ],
      });
    if (path === "/api/v1/admin/users/u1/mfa")
      return ok({
        factors: [factor],
        policy: { inherit_platform: true, minimum_factors: 0, required_factors: [] },
        allowed_factors: ["totp", "email"],
        effective_requirements: { minimum_factors: 0, required_factors: [] },
      });
    if (path === "/api/v1/admin/teams") return ok({ teams: [adminTeam] });
    if (path === "/api/v1/admin/teams/t1")
      return ok({ team: adminTeam, members: [member], quota_plan: plan, project_count: 1 });
    if (path === "/api/v1/admin/team-groups") return ok({ groups: [group] });
    if (path === "/api/v1/admin/quota-plans") return ok({ plans: [plan] });
    if (path === "/api/v1/admin/host-sources")
      return ok({
        sources: [
          {
            id: "hs1",
            label: "Wildcard",
            kind: "wildcard",
            base_domain: "sites.example.com",
            region: "local",
            enabled: true,
            allows_auto_assign: true,
            is_default: true,
            provider: null,
            config_keys: [],
            created_at: project.created_at,
          },
        ],
      });
    if (path === "/api/v1/admin/regional-ingresses")
      return ok({
        regional_ingresses: [
          {
            id: "ri1",
            region: "local",
            hostname: "entry.example.com",
            enabled: true,
            health_check_path: "/health",
            health_check_interval_seconds: 30,
            origin_host_preservation: true,
            dns_status: "resolved",
            healthy_nodes: [],
            node_statuses: [],
            created_at: project.created_at,
          },
        ],
      });
    if (path === "/api/v1/admin/announcements")
      return ok({ announcements: [announcement], pagination });
    if (path === "/api/v1/admin/codes")
      return ok({ scopes: ["registration"], codes: [], pagination });
    if (path === "/api/v1/admin/registration/emails") return ok({ emails: [] });
    if (path === "/api/v1/admin/reviews")
      return ok({
        total: 1,
        reviews: [
          {
            id: "r1",
            requested_at: project.created_at,
            deployment: {
              ...deployment,
              source_branch: "main",
              commit_hash: "abcdef",
              commit_message: "Review this release",
              serve_was_ready: true,
              release_status: "pending_review",
            },
            project,
            team,
            triggered_by: profile,
          },
        ],
      });
    if (path === "/api/v1/admin/projects")
      return ok({
        projects: [{ ...project, team, status: "active", latest_deployment: deployment }],
      });
    if (path === "/api/v1/admin/projects/p1")
      return ok({ project: { ...project, uuid: "p1", status: "active" }, team });
    if (path === "/api/v1/admin/projects/p1/deployments")
      return ok({ deployments: [{ ...deployment, commit_message: "Ship website" }] });
    if (path === "/api/v1/admin/projects/p1/domains")
      return ok({ domains: [{ ...host, review_status: "approved" }] });
    if (path === "/api/v1/admin/projects/p1/activity") return ok({ events: [], pagination });
    if (path === "/api/v1/admin/domain-https")
      return ok({ issuer: "letsencrypt", zerossl_eab_configured: false });
    if (path === "/api/v1/admin/storage")
      return ok({ storage, maintenance: false, migration: null });
    if (path === "/api/v1/admin/storage/test") return ok({ tested: true });
    if (path.startsWith("/api/v1/admin/cleanup/"))
      return ok(
        method === "DELETE"
          ? { deleted: 1, skipped: 0, failed: 0 }
          : { matched: 1, deletable: 1, skipped: 0, events: [], pagination, snapshot_before: 100 },
      );
    if (method !== "GET") return ok({ ok: true });
    return route.fulfill({
      status: 404,
      json: { code: 404, message: `Missing fixture: ${path}`, data: null },
    });
  });
  return calls;
}
export async function expectReady(page: Page) {
  await expect(
    page.getByRole("status").filter({ hasText: "Connecting to your workspace" }),
  ).toHaveCount(0);
  await expect(page.locator("gw-app")).toBeVisible();
}
