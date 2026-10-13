import { chromium, type Browser, type Page } from "@playwright/test";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import ts from "typescript";
import { accountSecurity, deployment, installApi, project, user } from "../browser/fixtures";

interface RouteEntry {
  pattern: string;
  tag: string;
  title: string;
  access: string;
}
interface CaptureEntry extends RouteEntry {
  id: string;
  url: string;
  stage?: string;
  state?: "setup-token" | "mfa-code" | "mfa-email" | "mfa-enroll" | "account-enroll";
}
interface Variant {
  id: string;
  label: string;
  width: number;
  height: number;
  theme: "light" | "dark";
}
interface ImageEntry {
  variant: string;
  label: string;
  file: string;
  width: number;
  height: number;
}
interface CaptureResult extends CaptureEntry {
  images: ImageEntry[];
}
const output = path.resolve(process.env.CONSOLE_SCREENSHOTS_DIR || "screenshots");
const baseURL = process.env.CONSOLE_SCREENSHOTS_URL || "http://127.0.0.1:4173";
const variants: Variant[] = [
  { id: "desktop-light", label: "Desktop - light", width: 1440, height: 960, theme: "light" },
  { id: "desktop-dark", label: "Desktop - dark", width: 1440, height: 960, theme: "dark" },
  { id: "mobile-light", label: "Mobile - light", width: 390, height: 844, theme: "light" },
];

async function routeInventory(): Promise<RouteEntry[]> {
  const source = ts.createSourceFile(
    "routes.ts",
    await readFile("src/ui/pages/index.ts", "utf8"),
    ts.ScriptTarget.Latest,
    true,
  );
  const entries: RouteEntry[] = [];
  const visit = (node: ts.Node): void => {
    if (ts.isCallExpression(node) && node.expression.getText(source) === "route") {
      const [pattern, tag, title, , access] = node.arguments;
      if (ts.isStringLiteral(pattern) && ts.isStringLiteral(tag) && ts.isStringLiteral(title)) {
        entries.push({
          pattern: pattern.text,
          tag: tag.text,
          title: title.text,
          access: access && ts.isStringLiteral(access) ? access.text : "user",
        });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return entries;
}

function concreteURL(pattern: string): string {
  const resourceId = pattern.includes("/quota-plans/")
    ? "q1"
    : pattern.includes("/team-groups/")
      ? "g1"
      : pattern.includes("/host-sources/")
        ? "hs1"
        : pattern.includes("/regional-ingresses/")
          ? "ri1"
          : pattern.includes("/users/")
            ? "u1"
            : pattern.includes("/teams/")
              ? "t1"
              : "n1";
  const parameters: Record<string, string> = {
    projectId: "p1",
    deploymentId: "d1",
    hostId: "h1",
    credentialId: "c1",
    userId: "u2",
    resourceId,
  };
  let url = pattern.replace(/:([a-zA-Z]+)/g, (_, name: string) => parameters[name] || name);
  if (url === "/reset-password") url += "?token=fixture-reset";
  if (url === "/verify-email") url += "?email=person%40example.com";
  if (url === "/invitations/accept") url += "?token=fixture-invitation";
  if (url === "/mfa") url += "#challenge=fixture-challenge";
  return url;
}

function captureInventory(routes: RouteEntry[]): CaptureEntry[] {
  const entries: Omit<CaptureEntry, "id">[] = [];
  for (const route of routes) {
    if (route.pattern === "/setup") {
      for (const stage of ["database", "admin", "site", "node", "storage", "finish"])
        entries.push({ ...route, title: `Setup - ${stage}`, url: "/setup", stage });
      entries.push({
        ...route,
        title: "Setup - node token handoff",
        url: "/setup",
        stage: "node",
        state: "setup-token",
      });
    } else if (route.pattern.includes(":section")) {
      for (const section of [
        "basic",
        "email",
        "authentication",
        "governance",
        "infrastructure",
        "runtime",
      ])
        entries.push({
          ...route,
          title: `Platform settings - ${section}`,
          url: route.pattern.replace(":section", section),
        });
    } else if (route.pattern.includes(":kind")) {
      for (const kind of ["regions", "https", "storage"])
        entries.push({
          ...route,
          title: `Infrastructure - ${kind}`,
          url: route.pattern.replace(":kind", kind),
        });
    } else {
      entries.push({ ...route, url: concreteURL(route.pattern) });
      if (route.pattern === "/mfa") {
        for (const [state, title] of [
          ["mfa-code", "MFA - authenticator verification"],
          ["mfa-email", "MFA - email verification"],
          ["mfa-enroll", "MFA - authenticator enrollment"],
        ] as const)
          entries.push({ ...route, title, url: concreteURL(route.pattern), state });
      }
      if (route.pattern === "/account/security/mfa/new")
        entries.push({
          ...route,
          title: "Account - authenticator enrollment",
          url: route.pattern,
          state: "account-enroll",
        });
    }
  }
  return entries.map((entry, index) => ({
    ...entry,
    id: `${String(index + 1).padStart(3, "0")}-${entry.title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/-$/, "")}`,
  }));
}

async function installScreenshotData(page: Page, entry: CaptureEntry): Promise<void> {
  await installApi(page, {
    loggedIn: entry.access === "user" || entry.access === "admin" || entry.access === "team-admin",
    setup: Boolean(entry.stage),
  });
  const created = project.created_at;
  const member = {
    id: "m2",
    user_id: "u2",
    email: "dev@example.com",
    display_name: "Morgan",
    role: "member",
    joined_at: created,
  };
  const audit = {
    id: "audit-1",
    actor_user_id: "u1",
    actor_node_id: null,
    actor_type: "user",
    team_id: "t1",
    visibility: "team",
    action: "project.deploy",
    target_type: "project",
    target_id: "p1",
    result: "success",
    reason: null,
    metadata: { environment: "production" },
    request_id: "fixture-request",
    source_ip: "192.0.2.1",
    user_agent: "Screenshot fixture",
    http_method: "POST",
    request_path: "/api/v1/projects/p1/deployments",
    status_code: 200,
    duration_ms: 24,
    changes: {},
    created_at: created,
  };
  const pagination = { page: 1, per_page: 25, total: 2, total_pages: 1 };
  const factor = {
    id: "f1",
    kind: "totp",
    verified: true,
    label: "Authenticator",
    created_at: created,
    last_used_at: created,
  };
  const notifications = [
    {
      id: "notice-1",
      action: "deployment.active",
      title: "Website is live",
      project: { id: "p1", name: "Website", slug: "website" },
      content: "Your production deployment is ready to receive traffic.",
      reason: null,
      target_url: "/projects/p1/deployments/d1",
      read_at: null,
      created_at: created,
    },
    {
      id: "notice-2",
      action: "announcement",
      announcement_id: "a1",
      title: "Welcome to your workspace",
      project: null,
      content: "Create a project, connect a repository, and deploy your first site.",
      reason: null,
      target_url: "/",
      read_at: created,
      created_at: created,
    },
  ];
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request(),
      uri = new URL(request.url()),
      pathname = uri.pathname;
    const ok = (data: unknown) => route.fulfill({ json: { code: 200, message: "OK", data } });
    if (pathname === "/api/v1/auth/providers")
      return ok({
        password_recovery_available: true,
        registration_email_verification: false,
        signup_policy: "open",
        password_policy: accountSecurity.password_policy,
      });
    if (pathname === "/api/v1/setup/state") return ok({ stage: entry.stage || "database" });
    if (pathname === "/api/v1/setup/node")
      return ok({
        node: { id: "n1", name: "local-node" },
        token: "EXAMPLE-NODE-TOKEN-FOR-SCREENSHOT-ONLY",
      });
    if (pathname === "/api/v1/auth/mfa/challenge")
      return ok({
        mfa_required: true,
        mfa_enrollment_required: entry.state === "mfa-enroll",
        challenge_token: "fixture-challenge",
        factors:
          entry.state === "mfa-enroll" ? [] : [factor, { ...factor, id: "f2", kind: "email" }],
        allowed_factors: ["totp", "email"],
        return_to: "/projects",
      });
    if (pathname === "/api/v1/me/security" && entry.url.startsWith("/account/security/mfa/new"))
      return ok({ ...accountSecurity, factors: [] });
    if (pathname === "/api/v1/teams/t1/members")
      return ok({
        members: [
          {
            ...member,
            id: "m1",
            user_id: "u1",
            email: user.email,
            display_name: user.display_name,
            role: "owner",
          },
          member,
        ],
      });
    if (pathname.endsWith("/ssh-host-keys"))
      return ok({
        host_keys: [
          {
            id: "ssh-1",
            host: "github.com",
            port: 22,
            key_type: "ssh-ed25519",
            fingerprint_sha256: "SHA256:EXAMPLE-FINGERPRINT-FOR-SCREENSHOT",
            status: "pending",
            approved_at: null,
            last_seen_at: created,
          },
        ],
      });
    if (pathname.endsWith("/audit-events") && !pathname.includes("/cleanup/"))
      return ok({
        events: [
          audit,
          {
            ...audit,
            id: "audit-2",
            action: "team.member.invite",
            target_type: "user",
            target_id: "u2",
          },
        ],
        pagination,
      });
    if (pathname === "/api/v1/notifications") return ok({ notifications, pagination });
    if (pathname.endsWith("/unread-count")) return ok({ count: 1 });
    if (pathname.endsWith("/quota/usage"))
      return ok({
        plan: { id: "q1", code: "default", name: "Team", source: "group" },
        usage: [
          { dimension: "projects", used: 3, limit: 10, period: "none" },
          { dimension: "members", used: 2, limit: 10, period: "none" },
          { dimension: "deployments.monthly", used: 24, limit: 100, period: "monthly" },
          { dimension: "build_minutes.monthly", used: 42, limit: 200, period: "monthly" },
        ],
      });
    if (pathname === "/api/v1/projects")
      return ok({
        projects: [
          project,
          {
            ...project,
            id: "p2",
            slug: "docs",
            name: "Documentation",
            repository_url: "https://github.com/acme/docs.git",
          },
          { ...project, id: "p3", slug: "api", name: "API service", runtime: "ssr" },
        ],
      });
    if (pathname === "/api/v1/projects/p1/deployments")
      return ok({
        deployments: [
          deployment,
          {
            ...deployment,
            id: "d2",
            environment: "preview",
            release_status: "approved",
            source: {
              ...deployment.source,
              branch: "feature/design",
              commit_message: "Update the homepage",
            },
          },
        ],
      });
    if (pathname === "/api/v1/projects/p1/deployments/d1")
      return ok({
        deployment,
        events: [
          {
            id: "event-1",
            kind: "build",
            message: "Build completed in 12 seconds",
            metadata: {},
            created_at: created,
          },
          {
            id: "event-2",
            kind: "release",
            message: "Production deployment activated",
            metadata: {},
            created_at: created,
          },
        ],
        artifacts: [
          {
            id: "artifact-1",
            kind: "static_site",
            storage_path: "fixture/site.tar",
            checksum_sha256: "EXAMPLE-CHECKSUM",
            size_bytes: 245760,
            manifest: {},
            created_at: created,
          },
        ],
        reviews: [
          {
            id: "review-1",
            status: "approved",
            reviewer_user_id: "u1",
            reason: "Ready for production",
            requested_at: created,
            reviewed_at: created,
          },
        ],
        review_required: false,
        was_active: true,
      });
    if (pathname.endsWith("/build-log"))
      return ok({
        lines: [
          "Cloning repository...",
          "Installing dependencies with Bun",
          "Building production assets",
          "Generated static output",
          "Deployment is ready",
        ].map((line, index) => ({
          seq: index + 1,
          stage: index < 2 ? "install" : "build",
          line,
          timestamp_ms: Date.parse(created) + index * 1000,
        })),
        last_seq: 5,
        build_status: "ready",
      });
    if (pathname === "/api/v1/admin/registration/emails")
      return ok({
        emails: [
          { id: "allow-1", email: "dev@example.com", created_at: created, created_by: user },
          { id: "allow-2", email: "design@example.com", created_at: created, created_by: user },
        ],
      });
    if (pathname === "/api/v1/admin/codes")
      return ok({
        scopes: ["registration"],
        codes: [
          {
            id: "code-1",
            code: "EXAMPLE-INVITE-01",
            scope: "registration",
            status: "available",
            expires_at: "2026-11-14T00:00:00Z",
            used_at: null,
            used_by: null,
            revoked_at: null,
            created_at: created,
          },
          {
            id: "code-2",
            code: "EXAMPLE-INVITE-02",
            scope: "registration",
            status: "used",
            expires_at: null,
            used_at: created,
            used_by: user,
            revoked_at: null,
            created_at: created,
          },
        ],
        pagination,
      });
    if (pathname.endsWith("/activity")) return ok({ events: [audit], pagination });
    return route.fallback();
  });
}

async function capture(
  browser: Browser,
  entry: CaptureEntry,
  variant: Variant,
): Promise<ImageEntry> {
  const context = await browser.newContext({
    baseURL,
    viewport: { width: variant.width, height: variant.height },
    deviceScaleFactor: 1,
    locale: "en-US",
    timezoneId: "Asia/Shanghai",
    reducedMotion: "reduce",
    serviceWorkers: "block",
  });
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("response", (response) => {
    if (response.url().includes("/api/v1/") && response.status() >= 404)
      errors.push(`API ${response.status()}: ${new URL(response.url()).pathname}`);
  });
  try {
    await page.addInitScript(
      (preference) => localStorage.setItem("grass-worker.theme", preference),
      variant.theme,
    );
    await installScreenshotData(page, entry);
    await page.goto(entry.url);
    await page.locator(entry.tag).waitFor({ state: "visible" });
    await page.waitForLoadState("networkidle");
    if (entry.state === "setup-token")
      await page.getByRole("button", { name: "Continue", exact: true }).click();
    if (
      entry.state === "mfa-code" ||
      entry.state === "mfa-enroll" ||
      entry.state === "account-enroll"
    )
      await page.getByRole("button", { name: /Authenticator app/ }).click();
    if (entry.state === "mfa-email") await page.getByRole("button", { name: /Email code/ }).click();
    if (entry.state?.startsWith("mfa-") || entry.state === "account-enroll")
      await page.getByLabel("Verification code").waitFor({ state: "visible" });
    if (entry.state === "mfa-enroll" || entry.state === "account-enroll")
      await page
        .getByRole("img", { name: "Authenticator setup QR code" })
        .waitFor({ state: "visible" });
    if (entry.state === "setup-token")
      await page
        .getByText("EXAMPLE-NODE-TOKEN-FOR-SCREENSHOT-ONLY", { exact: true })
        .waitFor({ state: "visible" });
    await page.evaluate(async () => {
      await document.fonts.ready;
      await Promise.all(
        [...document.images].map((image) =>
          image.complete ? Promise.resolve() : image.decode().catch(() => undefined),
        ),
      );
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    });
    await page.waitForFunction(
      () => !document.querySelector("gw-app")?.textContent?.includes("Loading page…"),
    );
    if (errors.length) throw new Error(errors.join("; "));
    const pageWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    if (pageWidth > variant.width)
      throw new Error(`Page overflows horizontally: ${pageWidth}px > ${variant.width}px`);
    const file = `${variant.id}/${entry.id}.png`;
    const height = await page.evaluate(() =>
      Math.max(document.documentElement.scrollHeight, innerHeight),
    );
    await page.screenshot({
      path: path.join(output, file),
      fullPage: true,
      animations: "disabled",
    });
    return { variant: variant.id, label: variant.label, file, width: variant.width, height };
  } catch (cause) {
    throw new Error(
      `${entry.url} / ${entry.state || "default"} / ${variant.id}: ${String(cause)}`,
      { cause },
    );
  } finally {
    await context.close();
  }
}

function gallery(results: CaptureResult[]): string {
  const data = JSON.stringify(results).replaceAll("<", "\\u003c");
  return `<!doctype html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Console Screenshot Gallery</title><style>
*{box-sizing:border-box}body{margin:0;background:#fafafa;color:#171717;font:14px/1.5 system-ui,sans-serif}header{position:sticky;top:0;background:white;border-bottom:1px solid #ddd;padding:20px 28px;z-index:2}h1{font-size:24px;margin:0 0 6px}p{margin:0;color:#666}.toolbar{display:flex;gap:12px;margin-top:16px;flex-wrap:wrap}input,select{font:inherit;padding:10px 12px;border:1px solid #ddd;border-radius:8px;background:white}input{min-width:260px;flex:1}.layout{display:grid;grid-template-columns:300px minmax(0,1fr);gap:24px;padding:24px}nav{max-height:calc(100vh - 185px);overflow:auto;position:sticky;top:170px}nav a{display:block;padding:8px 12px;text-decoration:none;color:#444;border-radius:6px}nav a:hover{background:#e5e5e5}nav small{display:block;font:11px monospace;color:#777;overflow-wrap:anywhere}.cards{display:grid;gap:28px}.card{border:1px solid #ddd;background:white;border-radius:10px;overflow:hidden}.caption{padding:16px 20px;border-bottom:1px solid #eee}.caption h2{font-size:16px;margin:0 0 4px}.caption code{font-size:12px;overflow-wrap:anywhere}.caption a{float:right;color:#171717}.card img{display:block;width:100%;height:auto}.card.mobile img{width:min(390px,100%);margin:auto}.card[hidden]{display:none}.count{font-size:12px;margin-top:12px;color:#666}@media(max-width:800px){.layout{grid-template-columns:1fr;padding:16px}nav{display:none}header{position:static;padding:16px}}
</style></head><body><header><h1>Console screenshot gallery</h1><p>Every route and key setup/MFA state. All screenshots use mock API data, not a live backend.</p><div class="toolbar"><input id="search" type="search" placeholder="Search by page title or route" aria-label="Search screenshots"><select id="variant" aria-label="Screenshot variant"><option value="desktop-light">Desktop - light (1440 x 960)</option><option value="desktop-dark">Desktop - dark (1440 x 960)</option><option value="mobile-light">Mobile - light (390 x 844)</option></select></div><div class="count" id="count"></div></header><div class="layout"><nav id="navigation" aria-label="Pages"></nav><main class="cards" id="cards"></main></div><script>
const entries=${data};const search=document.querySelector('#search'),variant=document.querySelector('#variant'),cards=document.querySelector('#cards'),navigation=document.querySelector('#navigation');
function render(){cards.replaceChildren();navigation.replaceChildren();const query=search.value.toLowerCase(),visible=entries.filter(entry=>(entry.title+' '+entry.url).toLowerCase().includes(query));document.querySelector('#count').textContent=visible.length+' pages - '+entries.reduce((count,entry)=>count+entry.images.length,0)+' screenshots';for(const entry of visible){const selected=entry.images.find(image=>image.variant===variant.value);const article=document.createElement('article');article.className='card'+(variant.value==='mobile-light'?' mobile':'');article.id=entry.id;const caption=document.createElement('div');caption.className='caption';const link=document.createElement('a');link.href=selected.file;link.target='_blank';link.textContent='Open full image';const title=document.createElement('h2');title.textContent=entry.title;const route=document.createElement('code');route.textContent=entry.url;caption.append(link,title,route);const image=document.createElement('img');image.src=selected.file;image.alt=entry.title+' - '+selected.label;image.loading='lazy';article.append(caption,image);cards.append(article);const nav=document.createElement('a');nav.href='#'+entry.id;const name=document.createElement('span');name.textContent=entry.title;const uri=document.createElement('small');uri.textContent=entry.url;nav.append(name,uri);navigation.append(nav)}}search.addEventListener('input',render);variant.addEventListener('change',render);render();
</script></body></html>`;
}

const routes = await routeInventory();
const entries = captureInventory(routes);
await mkdir(output, { recursive: true });
for (const variant of variants) await mkdir(path.join(output, variant.id), { recursive: true });
const browser = await chromium.launch();
const results: CaptureResult[] = entries.map((entry) => ({ ...entry, images: [] }));
let index = 0,
  completed = 0;
try {
  await Promise.all(
    Array.from({ length: 3 }, async () => {
      while (index < results.length) {
        const entry = results[index++];
        for (const variant of variants) entry.images.push(await capture(browser, entry, variant));
        completed++;
        console.log(`${completed}/${results.length} ${entry.title} (${entry.url})`);
      }
    }),
  );
} finally {
  await browser.close();
}
const covered = new Set(results.map((entry) => entry.pattern));
if (routes.some((route) => !covered.has(route.pattern)))
  throw new Error("The gallery does not cover every route.");
await writeFile(
  path.join(output, "manifest.json"),
  JSON.stringify(
    {
      generatedAt: new Date().toISOString(),
      mockData: true,
      routeCount: routes.length,
      pageCount: results.length,
      screenshotCount: results.reduce((count, entry) => count + entry.images.length, 0),
      variants,
      entries: results,
    },
    null,
    2,
  ) + "\n",
);
await writeFile(path.join(output, "index.html"), gallery(results));
console.log(`Saved ${results.length} page/state sets to ${output}`);
