import { expect, test } from "@playwright/test";
import { accountSecurity, installApi } from "./fixtures";

const routes = [
  ["/", "gw-page-dashboard"],
  ["/projects", "gw-page-projects"],
  ["/projects/new", "gw-page-project-new"],
  ["/projects/p1", "gw-page-project-overview"],
  ["/projects/p1/deployments", "gw-page-deployments"],
  ["/projects/p1/deployments/new", "gw-page-deployments"],
  ["/projects/p1/deployments/d1", "gw-page-deployment-detail"],
  ["/projects/p1/domains", "gw-page-domains"],
  ["/projects/p1/domains/new", "gw-page-domains"],
  ["/projects/p1/domains/h1", "gw-page-domains"],
  ["/projects/p1/domains/h1/certificate/import", "gw-page-domains"],
  ["/projects/p1/settings", "gw-page-project-settings"],
  ["/projects/p1/settings/build-and-deployment", "gw-page-project-build"],
  ["/quota", "gw-page-quota"],
  ["/notifications", "gw-page-notifications"],
  ["/account/profile", "gw-page-profile"],
  ["/account/security", "gw-page-security"],
  ["/account/security/password", "gw-page-security"],
  ["/account/security/mfa/new", "gw-page-security"],
  ["/teams/new", "gw-page-team"],
  ["/settings/team", "gw-page-team"],
  ["/settings/team/source-access", "gw-page-source-access"],
  ["/settings/team/source-access/new", "gw-page-source-access"],
  ["/settings/team/source-access/c1", "gw-page-source-access"],
  ["/settings/members", "gw-page-members"],
  ["/settings/members/new", "gw-page-members"],
  ["/settings/members/u1", "gw-page-members"],
  ["/settings/audit", "gw-page-audit"],
  ["/admin/reviews", "gw-page-admin-reviews"],
  ["/admin/reviews/d1", "gw-page-admin-reviews"],
  ["/admin/projects", "gw-page-admin-projects"],
  ["/admin/projects/p1", "gw-page-admin-projects"],
  ["/admin/nodes", "gw-page-admin-nodes"],
  ["/admin/nodes/new", "gw-page-admin-nodes"],
  ["/admin/nodes/n1", "gw-page-admin-nodes"],
  ["/admin/nodes/n1/delete", "gw-page-admin-nodes"],
  ["/admin/host-sources", "gw-page-admin-host-sources"],
  ["/admin/host-sources/new", "gw-page-admin-host-sources"],
  ["/admin/host-sources/hs1", "gw-page-admin-host-sources"],
  ["/admin/regional-ingresses", "gw-page-admin-ingresses"],
  ["/admin/regional-ingresses/new", "gw-page-admin-ingresses"],
  ["/admin/regional-ingresses/ri1", "gw-page-admin-ingresses"],
  ["/admin/quota-plans", "gw-page-admin-quota-plans"],
  ["/admin/quota-plans/new", "gw-page-admin-quota-plans"],
  ["/admin/quota-plans/q1", "gw-page-admin-quota-plans"],
  ["/admin/team-groups", "gw-page-admin-team-groups"],
  ["/admin/team-groups/new", "gw-page-admin-team-groups"],
  ["/admin/team-groups/g1", "gw-page-admin-team-groups"],
  ["/admin/users", "gw-page-admin-users"],
  ["/admin/users/new", "gw-page-admin-users"],
  ["/admin/users/u1", "gw-page-admin-users"],
  ["/admin/teams", "gw-page-admin-teams"],
  ["/admin/teams/new", "gw-page-admin-teams"],
  ["/admin/teams/t1", "gw-page-admin-teams"],
  ["/admin/registration", "gw-page-admin-registration"],
  ["/admin/code", "gw-page-admin-codes"],
  ["/admin/code/new", "gw-page-admin-codes"],
  ["/admin/settings/basic", "gw-page-admin-settings"],
  ["/admin/settings/email", "gw-page-admin-settings"],
  ["/admin/settings/authentication", "gw-page-admin-settings"],
  ["/admin/settings/governance", "gw-page-admin-settings"],
  ["/admin/settings/infrastructure", "gw-page-admin-settings"],
  ["/admin/settings/runtime", "gw-page-admin-settings"],
  ["/admin/settings/announcements", "gw-page-admin-announcements"],
  ["/admin/settings/announcements/new", "gw-page-admin-announcements"],
  ["/admin/settings/infrastructure/regions", "gw-page-admin-infrastructure"],
  ["/admin/settings/infrastructure/https", "gw-page-admin-infrastructure"],
  ["/admin/settings/infrastructure/storage", "gw-page-admin-infrastructure"],
  ["/admin/audit", "gw-page-audit"],
  ["/admin/cleanup", "gw-page-admin-cleanup"],
] as const;

for (const [path, tag] of routes) {
  test(`opens ${path} directly`, async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await installApi(page);
    await page.goto(path);
    await expect(page.locator(tag)).toBeVisible();
    await expect(page.getByText("Missing fixture:", { exact: false })).toHaveCount(0);
    await expect(page.getByText("Something went wrong", { exact: true })).toHaveCount(0);
    await expect(page.locator("gw-spinner")).toHaveCount(0);
    expect(errors).toEqual([]);
  });
}

test("signs in with Enter and preserves a local return destination", async ({ page }) => {
  const calls = await installApi(page, { loggedIn: false });
  await page.goto("/login?return_to=%2Fprojects");
  await page.getByLabel("Email", { exact: true }).fill("person@example.com");
  await page.getByLabel("Password", { exact: true }).fill("  keep spaces  ");
  await page.getByLabel("Password", { exact: true }).press("Enter");
  await expect(page).toHaveURL(/\/projects$/);
  await expect(page.locator("gw-page-projects")).toBeVisible();
  expect(calls.find((call) => call.path.endsWith("/login"))?.body).toMatchObject({
    email: "person@example.com",
    password: "  keep spaces  ",
    return_to: "/projects",
  });
});

test("shows and hides the password without changing its value", async ({ page }) => {
  await installApi(page, { loggedIn: false });
  await page.goto("/login");
  const password = page.getByLabel("Password", { exact: true });
  await password.fill("correct horse");
  await page.getByRole("button", { name: "Show password" }).click();
  await expect(password).toHaveAttribute("type", "text");
  await expect(password).toHaveValue("correct horse");
  await page.getByRole("button", { name: "Hide password" }).click();
  await expect(password).toHaveAttribute("type", "password");
});

test("completes MFA through a clear method and code flow", async ({ page }) => {
  const calls = await installApi(page, { loggedIn: false, mfa: true });
  await page.goto("/login");
  await page.getByLabel("Email", { exact: true }).fill("person@example.com");
  await page.getByLabel("Password", { exact: true }).fill("password123");
  await page.getByRole("button", { name: "Login", exact: true }).click();
  await expect(page).toHaveURL(/\/mfa#challenge=/);
  await page.getByRole("button", { name: /Authenticator app/ }).click();
  await expect(page.getByLabel("Verification code")).toBeFocused();
  await page.getByLabel("Verification code").fill("123456");
  await page.getByRole("button", { name: "Verify and continue" }).click();
  await expect(page).toHaveURL(/\/projects$/);
  expect(calls.find((call) => call.path.endsWith("/mfa/verify"))?.body).toMatchObject({
    challenge_token: "fixture-challenge",
    factor_id: "f1",
    code: "123456",
  });
});

test("protects platform administration and team audit routes", async ({ page }) => {
  await installApi(page, { role: "user", teamRole: "viewer" });
  await page.goto("/admin/nodes");
  await expect(page).toHaveURL(/\/$/);
  await page.goto("/settings/audit");
  await expect(page).toHaveURL(/\/$/);
  await page.goto("/projects/new");
  await expect(page.getByText("Select a team where you have permission")).toBeVisible();
});

test("saves profile through the unified API with CSRF", async ({ page }) => {
  const calls = await installApi(page);
  await page.goto("/account/profile");
  await page.getByLabel("Display name").fill("Updated name");
  await page.getByRole("button", { name: "Save profile" }).click();
  await expect(page.getByText("Saved successfully.", { exact: true })).toBeVisible();
  const update = calls.find((call) => call.path === "/api/v1/me" && call.method === "PATCH");
  expect(update?.body).toEqual({ display_name: "Updated name" });
  expect(update?.headers["x-csrf-token"]).toBe("fixture-csrf");
});

test("requires a preview and confirmation before cleanup", async ({ page }) => {
  const calls = await installApi(page);
  await page.goto("/admin/cleanup");
  await expect(page.getByRole("button", { name: "Delete eligible data" })).toHaveCount(0);
  await page.getByRole("button", { name: "Preview cleanup" }).click();
  await page.getByRole("button", { name: "Delete eligible data" }).click();
  expect(calls.some((call) => call.method === "DELETE")).toBe(false);
  await page.getByRole("button", { name: "Delete data", exact: true }).click();
  await expect(page.getByText("Cleanup result", { exact: true })).toBeVisible();
  expect(calls.find((call) => call.method === "DELETE")?.body).toMatchObject({
    snapshot_before: 100,
  });
});

test("theme transition uses a 300 ms spring circle and saves preference", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("grass-worker.theme", "light"));
  await installApi(page);
  await page.goto("/projects");
  await expect(page.locator("gw-page-projects")).toBeVisible();
  await page.evaluate(() => {
    const animate = Element.prototype.animate;
    const records: { frames: unknown; options: unknown }[] = [];
    Object.assign(window, { animationRecords: records });
    Element.prototype.animate = function (frames, options) {
      records.push({ frames, options });
      return animate.call(this, frames, options);
    };
  });
  await page.getByRole("button", { name: "Toggle theme", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("html")).not.toHaveClass(/theme-transition/);
  expect(await page.evaluate(() => localStorage.getItem("grass-worker.theme"))).toBe("dark");
  const records = await page.evaluate(
    () =>
      (
        window as unknown as {
          animationRecords: {
            frames: { clipPath: string }[];
            options: { duration: number; pseudoElement: string };
          }[];
        }
      ).animationRecords,
  );
  if (await page.evaluate(() => Boolean(document.startViewTransition))) {
    expect(records).toHaveLength(1);
    expect(records[0].options).toMatchObject({
      duration: 300,
      pseudoElement: "::view-transition-new(root)",
    });
    expect(records[0].frames).toHaveLength(61);
    expect(records[0].frames[0].clipPath).toMatch(/^circle\(0px at /);
    expect(records[0].frames[30].clipPath).toContain("circle(");
  }
});

test("reduced motion switches theme without an animation", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce", colorScheme: "light" });
  await installApi(page);
  await page.goto("/projects");
  await page.getByRole("button", { name: "Toggle theme", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("html")).not.toHaveClass(/theme-transition/);
});

for (const width of [390, 768, 1280, 1567, 1920]) {
  test(`login and console fit a ${width}px viewport`, async ({ page }) => {
    await page.setViewportSize({ width, height: width === 1567 ? 1018 : 900 });
    await installApi(page, { loggedIn: false });
    await page.goto("/login");
    await expect(page.getByRole("button", { name: "Login", exact: true })).toBeVisible();
    const input = await page.locator("gw-input[name=email]").boundingBox();
    expect(input!.width).toBeLessThanOrEqual(560);
    expect(Math.abs(input!.x + input!.width / 2 - width / 2)).toBeLessThan(2);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      width,
    );
    await page.getByLabel("Email", { exact: true }).fill("person@example.com");
    await page.getByLabel("Password", { exact: true }).fill("password123");
    await page.getByRole("button", { name: "Login", exact: true }).click();
    await expect(page.locator("gw-page-dashboard")).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      width,
    );
  });
}

for (const [path, tag] of [
  ["/signup", "gw-page-signup"],
  ["/forgot-password", "gw-page-forgot-password"],
  ["/reset-password?token=fixture-reset", "gw-page-reset-password"],
  ["/verify-email?email=person%40example.com", "gw-page-verify-email"],
  ["/invitations/accept?token=fixture-invitation", "gw-page-invitation"],
]) {
  test(`opens public flow ${path}`, async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await installApi(page, { loggedIn: false });
    await page.goto(path);
    await expect(page.locator(tag)).toBeVisible();
    expect(errors).toEqual([]);
  });
}

test("opens setup without trying to restore a database-backed session", async ({ page }) => {
  const calls = await installApi(page, { loggedIn: false, setup: true });
  await page.goto("/");
  await expect(page).toHaveURL(/\/setup$/);
  await expect(page.getByLabel("Database host")).toBeVisible();
  expect(calls.some((call) => call.path === "/api/v1/me")).toBe(false);
});

test("submits project creation with framework settings", async ({ page }) => {
  await installApi(page);
  let submitted: Record<string, unknown> | null = null;
  await page.route("**/api/v1/projects", async (route) => {
    if (route.request().method() !== "POST") return route.fallback();
    submitted = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({
      json: {
        code: 200,
        message: "OK",
        data: { project: { id: "p1" }, host_assignment: { assigned: true } },
      },
    });
  });
  await page.goto("/projects/new");
  await page.getByLabel("Project name", { exact: true }).fill("New Website");
  await expect(page.getByLabel("Project slug")).toHaveValue("new-website");
  await page.getByLabel("Git repository URL").fill("https://github.com/acme/site.git");
  await page.getByRole("button", { name: "Create project", exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/p1$/);
  expect(submitted).toMatchObject({
    team_id: "t1",
    name: "New Website",
    slug: "new-website",
    runtime: "static",
    framework_hint: "vite",
    output_directory: "dist",
    default_branch: "main",
  });
});

test("saves quota limits as numbers or unlimited", async ({ page }) => {
  const calls = await installApi(page);
  await page.goto("/admin/quota-plans/q1");
  await page
    .locator("gw-page-admin-quota-plans")
    .getByLabel("Projects", { exact: true })
    .fill("12");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page).toHaveURL(/\/admin\/quota-plans$/);
  expect(
    calls.find((call) => call.method === "PATCH" && call.path.endsWith("/quota-plans/q1"))?.body,
  ).toMatchObject({
    limits: expect.arrayContaining([
      { dimension: "projects", limit_value: 12 },
      { dimension: "members", limit_value: null },
    ]),
  });
});

test("saves SMTP without replacing a blank write-only password", async ({ page }) => {
  const calls = await installApi(page);
  await page.goto("/admin/settings/email");
  await page.getByLabel("From name").fill("Updated sender");
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByText("Saved successfully.", { exact: true })).toBeVisible();
  const body = calls.find(
    (call) => call.path === "/api/v1/admin/settings" && call.method === "PATCH",
  )?.body as Record<string, unknown>;
  expect(body.mail_from_name).toBe("Updated sender");
  expect(body).not.toHaveProperty("mail_smtp_password");
});

test("imports a certificate from a dedicated page", async ({ page }) => {
  const calls = await installApi(page);
  await page.goto("/projects/p1/domains/h1/certificate/import");
  await page
    .getByLabel("Certificate PEM")
    .fill("-----BEGIN CERTIFICATE-----\nfixture\n-----END CERTIFICATE-----");
  await page
    .getByLabel("Private key PEM")
    .fill("-----BEGIN PRIVATE KEY-----\nfixture\n-----END PRIVATE KEY-----");
  await page.getByRole("button", { name: "Import certificate", exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/p1\/domains\/h1$/);
  expect(calls.find((call) => call.path.endsWith("/certificate/import"))?.body).toMatchObject({
    certificate_pem: expect.stringContaining("BEGIN CERTIFICATE"),
    private_key_pem: expect.stringContaining("BEGIN PRIVATE KEY"),
  });
});

test("discards a cleanup preview when filters change during its request", async ({ page }) => {
  await installApi(page);
  let release: (() => void) | undefined;
  await page.route("**/api/v1/admin/cleanup/audit-events**", async (route) => {
    await new Promise<void>((resolve) => {
      release = resolve;
    });
    return route.fulfill({
      json: {
        code: 200,
        message: "OK",
        data: {
          matched: 1,
          deletable: 1,
          skipped: 0,
          events: [],
          pagination: { page: 1, total_pages: 1 },
          snapshot_before: 100,
        },
      },
    });
  });
  await page.goto("/admin/cleanup");
  await page.getByRole("button", { name: "Preview cleanup" }).click();
  await expect.poll(() => Boolean(release)).toBe(true);
  await page.getByLabel("Action", { exact: true }).fill("different.action");
  release!();
  await expect(page.getByRole("button", { name: "Preview cleanup" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "Delete eligible data" })).toHaveCount(0);
});

test("adds an authenticator through the dedicated enrollment page", async ({ page }) => {
  const calls = await installApi(page);
  await page.route("**/api/v1/me/security", (route) =>
    route.fulfill({
      json: {
        code: 200,
        message: "OK",
        data: {
          ...accountSecurity,
          factors: [],
          allowed_factors: ["totp", "email"],
          mail_available: true,
          email_verified: true,
        },
      },
    }),
  );
  await page.goto("/account/security/mfa/new");
  await page.getByRole("button", { name: "Authenticator app", exact: true }).click();
  await expect(page.getByRole("img", { name: "Authenticator setup QR code" })).toHaveAttribute(
    "src",
    /^data:image\/png;base64,/,
  );
  await page.getByLabel("Verification code").fill("123456");
  await page.getByRole("button", { name: "Verify and add" }).click();
  await expect(page).toHaveURL(/\/account\/security$/);
  await expect(page.getByRole("heading", { name: "Account security", exact: true })).toBeVisible();
  expect(calls.find((call) => call.path.endsWith("/mfa/f1/confirm"))?.body).toEqual({
    code: "123456",
  });
});

test("does not offer an unavailable email enrollment method", async ({ page }) => {
  await installApi(page);
  await page.route("**/api/v1/me/security", (route) =>
    route.fulfill({
      json: {
        code: 200,
        message: "OK",
        data: {
          ...accountSecurity,
          factors: [],
          allowed_factors: ["totp", "email"],
          mail_available: false,
          email_verified: false,
        },
      },
    }),
  );
  await page.goto("/account/security/mfa/new");
  await expect(page.getByRole("button", { name: "Authenticator app", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Email code", exact: true })).toHaveCount(0);
});
