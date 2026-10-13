import { expect, test, type Page } from "@playwright/test";
import { installApi } from "./fixtures";

async function expectAuthGeometry(page: Page, width: number) {
  const form = page.locator(".auth-form");
  const submit = form.locator('gw-button[type="submit"]');
  await expect(submit).toBeVisible();
  const controls = form.locator(".control");
  const heights = await controls.evaluateAll((elements) =>
    elements.map((element) => element.getBoundingClientRect().height),
  );
  expect(heights.length).toBeGreaterThan(0);
  expect(heights.every((height) => height === 48)).toBe(true);
  const formBox = (await form.boundingBox())!;
  const buttonBox = (await submit.boundingBox())!;
  expect(buttonBox.height).toBe(48);
  if (width > 600) {
    expect(buttonBox.width).toBeGreaterThanOrEqual(148);
    expect(buttonBox.width).toBeLessThan(formBox.width);
    expect(
      Math.abs(buttonBox.x + buttonBox.width / 2 - formBox.x - formBox.width / 2),
    ).toBeLessThan(1);
  } else {
    expect(buttonBox.width).toBe(formBox.width);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
    width,
  );
}

for (const width of [390, 1280, 1920]) {
  test(`authentication controls and submissions are consistent at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 960 });
    await installApi(page, { loggedIn: false, mfaEmail: true });
    for (const path of [
      "/login",
      "/signup",
      "/forgot-password",
      "/reset-password?token=fixture-reset",
      "/verify-email?email=person%40example.com",
    ]) {
      await page.goto(path);
      await expectAuthGeometry(page, width);
    }
    await page.goto("/mfa#challenge=fixture-challenge");
    await page.getByRole("button", { name: /Authenticator app/ }).click();
    await expectAuthGeometry(page, width);
    await page.getByRole("button", { name: "Use another method" }).click();
    await page.getByRole("button", { name: /Email code/ }).click();
    await expectAuthGeometry(page, width);
  });
}

test("registration keeps invite-code inputs at the primary height", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 960 });
  await installApi(page, { loggedIn: false });
  await page.route("**/api/v1/auth/providers", (route) =>
    route.fulfill({
      json: {
        code: 200,
        message: "OK",
        data: {
          signup_policy: "invite_only",
          password_policy: { min_length: 8, max_length: 1024 },
        },
      },
    }),
  );
  await page.goto("/signup");
  await expect(page.getByLabel("Registration code")).toBeVisible();
  await expectAuthGeometry(page, 1280);
});

test("authentication settings expose password and MFA configuration", async ({ page }) => {
  await installApi(page);
  await page.goto("/admin/settings/authentication");
  await expect(page.getByRole("button", { name: "Save password policy" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Save MFA policy" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Manage identity providers" })).toHaveCount(0);
});

for (const width of [390, 768, 1280, 1920]) {
  test(`project creation restores the split workspace at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 960 });
    await installApi(page);
    await page.goto("/projects/new");
    const details = page.getByRole("region", { name: "Configure Project" });
    const presets = page.getByRole("region", { name: "Framework Presets" });
    await expect(details).toBeVisible();
    await expect(presets).toBeVisible();
    await expect(page.getByRole("link", { name: "Back", exact: true })).toHaveAttribute(
      "href",
      "/projects",
    );
    await expect(page.getByRole("button", { name: "Toggle navigation" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Account menu", exact: true })).toBeVisible();
    await expect(page.locator(".project-create-preset")).toHaveCount(10);
    const left = (await details.boundingBox())!;
    const right = (await presets.boundingBox())!;
    if (width >= 1024) {
      expect(left.y).toBe(right.y);
      expect(right.x).toBe(left.x + left.width);
      expect(left.width + right.width).toBeLessThanOrEqual(1440);
    } else {
      expect(right.y).toBeGreaterThanOrEqual(left.y + left.height);
      expect(right.x).toBe(left.x);
    }
    const input = await page.locator('gw-input[name="name"] .control').boundingBox();
    const submit = await page
      .getByRole("button", { name: "Create project", exact: true })
      .boundingBox();
    expect(submit!.width).toBe(input!.width);
    await page.getByText("Build settings", { exact: true }).click();
    await expect(page.getByLabel("Build command", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      width,
    );
  });
}

test("framework selection preserves project details and submits customized build settings", async ({
  page,
}) => {
  const calls = await installApi(page);
  await page.goto("/projects/new");
  await page.getByLabel("Project name", { exact: true }).fill("New Website");
  await page.getByLabel("Project slug", { exact: true }).fill("custom-slug");
  await page.getByLabel("Git repository URL").fill("git@github.com:acme/site.git");
  await page.getByRole("button", { name: /Next.js \(SSR\)/ }).click();
  await expect(page.getByRole("button", { name: /Next.js \(SSR\)/ })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.getByLabel("Project name", { exact: true })).toHaveValue("New Website");
  await expect(page.getByLabel("Project slug", { exact: true })).toHaveValue("custom-slug");
  await page.getByText("Build settings", { exact: true }).click();
  await expect(page.getByLabel("Runtime", { exact: true })).toHaveValue("ssr");
  await page.getByLabel("Root directory", { exact: true }).fill("web");
  await page.getByLabel("Build command", { exact: true }).fill("bun run build");
  await page.getByRole("button", { name: "Create project", exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/p1$/);
  expect(
    calls.find((call) => call.path === "/api/v1/projects" && call.method === "POST")?.body,
  ).toMatchObject({
    team_id: "t1",
    name: "New Website",
    slug: "custom-slug",
    repository_url: "git@github.com:acme/site.git",
    runtime: "ssr",
    framework_hint: "nextjs-ssr",
    root_directory: "web",
    install_command: "npm install",
    build_command: "bun run build",
  });
});
