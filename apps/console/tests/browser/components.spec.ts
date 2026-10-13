import { expect, test } from "@playwright/test";
import { installApi } from "./fixtures";

test.beforeEach(async ({ page }) => {
  await installApi(page, { loggedIn: false });
  await page.goto("/ui");
  await expect(page.locator("gw-page-components")).toBeVisible();
});

test("form-associated controls participate in FormData and reset", async ({ page }) => {
  await page.getByLabel("Email", { exact: true }).fill("person@example.com");
  await page.getByLabel("Password", { exact: true }).fill("keep spaces ");
  await page.getByRole("checkbox", { name: "Agree to terms" }).check();
  await page.getByRole("switch", { name: "Enable notifications" }).check();
  await page.getByLabel("Select", { exact: true }).selectOption("two");
  const values = await page
    .locator("gw-page-components form")
    .evaluate((form) => Object.fromEntries(new FormData(form as HTMLFormElement)));
  expect(values).toMatchObject({
    email: "person@example.com",
    password: "keep spaces ",
    agree: "on",
    enabled: "on",
    select: "two",
  });
  await page.getByRole("button", { name: "Reset", exact: true }).click();
  await expect(page.getByLabel("Email", { exact: true })).toHaveValue("");
  await expect(page.getByRole("checkbox", { name: "Agree to terms" })).not.toBeChecked();
});

test("native validation prevents submission and focuses invalid control", async ({ page }) => {
  await page.getByRole("button", { name: "Submit form" }).click();
  await expect(page.getByLabel("Email", { exact: true })).toBeFocused();
  await expect(page.getByText('"email"', { exact: false })).toHaveCount(0);
});

test("single-line controls and password visibility buttons share the primary height", async ({
  page,
}) => {
  const controls = page.locator(
    "gw-page-components gw-input .control, gw-password-input .control, gw-select .control, gw-combobox .control, gw-input-otp .control",
  );
  await expect(controls).toHaveCount(7);
  expect(
    await controls.evaluateAll((elements) =>
      elements.map((element) => element.getBoundingClientRect().height),
    ),
  ).toEqual(Array(7).fill(48));
  const reveal = await page.getByRole("button", { name: "Show password" }).boundingBox();
  expect(reveal!.height).toBe(48);
  expect(reveal!.width).toBe(48);
  await page.getByRole("button", { name: "Show password" }).click();
  expect((await page.getByRole("button", { name: "Hide password" }).boundingBox())!.height).toBe(
    48,
  );
  expect(
    (await page.getByLabel("Textarea", { exact: true }).boundingBox())!.height,
  ).toBeGreaterThan(48);
});

test("dialog traps focus and restores it after Escape", async ({ page }) => {
  const trigger = page.getByRole("button", { name: "Open dialog", exact: true });
  await trigger.click();
  await expect(page.getByRole("dialog", { name: "Example dialog" })).toBeVisible();
  await page.keyboard.press("Tab");
  const inDialog = await page.evaluate(() => {
    let active: Element | null = document.activeElement;
    while (active?.shadowRoot?.activeElement) active = active.shadowRoot.activeElement;
    return Boolean(
      active?.closest("dialog") ||
      (active?.getRootNode() instanceof ShadowRoot &&
        (active.getRootNode() as ShadowRoot).host.closest("gw-dialog")),
    );
  });
  expect(inDialog).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Example dialog" })).not.toBeVisible();
  await expect(trigger).toBeFocused();
});

test("tabs support arrow keys and associate panels", async ({ page }) => {
  await page.getByRole("tab", { name: "First tab" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Second tab" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByRole("tabpanel", { name: "Second tab" })).toBeVisible();
  await expect(page.getByRole("tabpanel", { name: "First tab" })).not.toBeVisible();
});

test("dropdown menus navigate with arrow keys and close after selection", async ({ page }) => {
  await page.getByRole("button", { name: "Example menu", exact: true }).click();
  const host = page.locator('gw-dropdown-menu[label="Example menu"]');
  const menu = host.getByRole("menu", { name: "Example menu" });
  await expect(menu).toBeVisible();
  await host.getByRole("menuitem", { name: "Overview" }).focus();
  await page.keyboard.press("ArrowDown");
  await expect(host.getByRole("menuitem", { name: "Projects" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(menu).not.toBeVisible();
});

test("component gallery keeps table overflow inside a mobile viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator("gw-page-components")).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  const table = page.getByRole("region", { name: "Results", exact: true });
  expect(await table.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);
});
