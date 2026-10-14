import { apiUrl } from "@/infra/api";
import { StoreController } from "@/infra/controllers/store";
import { QueryController, queryClient } from "@/infra/query";
import { listenNavigation, location, matchPath, navigate } from "@/infra/router";
import { errorMessage } from "@/infra/serializer";
import { theme, watchSystemTheme } from "@/infra/theme";
import { listenSession, restoreSession, session, sessionContext } from "@/services/auth/state";
import { branding, refreshBranding } from "@/services/branding/state";
import { clearTeams, refreshTeams, teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { redirects, routes, type RouteDefinition } from "@/ui/pages";
import type { Page } from "@/ui/pages/base";
import { ContextProvider } from "@lit/context";
import { LitElement, html } from "lit";
interface Health {
  status: string;
  service: string;
  version: string;
  setup?: boolean;
}
async function health(): Promise<Health> {
  const response = await fetch(apiUrl("/health"), { credentials: "include" });
  if (!response.ok) throw new Error("The Control API is unavailable.");
  const data = (await response.json()) as Health;
  if (typeof data.status !== "string") throw new Error("Invalid health response.");
  return data;
}
export class App extends LitElement {
  static properties = {
    page: { state: true },
    route: { state: true },
    loading: { state: true },
    failure: { state: true },
  };
  page: Page | null = null;
  route: RouteDefinition | null = null;
  loading = false;
  failure = "";
  private health = new QueryController(this, {
    queryKey: ["health"],
    queryFn: health,
    retry: 2,
    refetchInterval: 3000,
  });
  private auth = new StoreController(this, session);
  private teams = new StoreController(this, teams);
  private location = new StoreController(this, location);
  private brand = new StoreController(this, branding);
  private theme = new StoreController(this, theme);
  private provider = new ContextProvider(this, { context: sessionContext, initialValue: session });
  private cleanups: (() => void)[] = [];
  private started = false;
  private userId: string | null = null;
  private key = "";
  private version = 0;
  protected createRenderRoot() {
    return this;
  }
  connectedCallback() {
    super.connectedCallback();
    queryClient.mount();
    this.cleanups = [listenNavigation(), listenSession(), watchSystemTheme()];
    void refreshBranding();
  }
  disconnectedCallback() {
    this.version++;
    this.cleanups.forEach((cleanup) => cleanup());
    queryClient.unmount();
    super.disconnectedCallback();
  }
  protected updated() {
    queueMicrotask(() => this.synchronize());
  }
  private synchronize() {
    if (!this.isConnected) return;
    const health = this.health.result.data;
    if (!health) return;
    if (health.setup) {
      if (this.location.value.pathname !== "/setup") navigate("/setup", true);
      void this.resolve();
      return;
    }
    if (!this.started) {
      this.started = true;
      void restoreSession();
    }
    const user = this.auth.value.user,
      id = user?.id || null;
    if (id !== this.userId) {
      this.userId = id;
      if (id) void refreshTeams();
      else clearTeams();
    }
    if (this.location.value.pathname === "/setup") navigate("/login", true);
    if (!this.auth.value.loading) void this.resolve();
  }
  private async resolve() {
    const location = this.location.value,
      path = location.pathname;
    const redirect = redirects[path];
    if (redirect) {
      navigate(redirect, true);
      return;
    }
    let route: RouteDefinition | undefined,
      params: Record<string, string> | null = null;
    for (const candidate of routes) {
      const matched = matchPath(candidate.pattern, path);
      if (matched) {
        route = candidate;
        params = matched;
        break;
      }
    }
    if (
      params?.section &&
      !["basic", "email", "authentication", "governance", "infrastructure", "runtime"].includes(
        params.section,
      )
    )
      route = undefined;
    if (params?.kind && !["regions", "https", "storage"].includes(params.kind)) route = undefined;
    const user = this.auth.value.user;
    if (route && !["public", "guest"].includes(route.access) && !user) {
      navigate(
        `/login?${new URLSearchParams({ return_to: path + (location.search.size ? `?${location.search}` : "") })}`,
        true,
      );
      return;
    }
    if (route?.access === "guest" && user) {
      navigate("/", true);
      return;
    }
    if (route?.access === "admin" && user?.platform_role !== "admin") {
      navigate("/", true);
      return;
    }
    if (route?.access === "team-admin") {
      if (this.teams.value.loading) return;
      if (!["owner", "admin"].includes(this.teams.value.role || "")) {
        navigate("/", true);
        return;
      }
    }
    const key = [
      path,
      location.search.toString(),
      location.hash,
      user?.id,
      this.teams.value.active?.id,
      this.teams.value.role,
    ].join("|");
    if (key === this.key) return;
    this.key = key;
    const version = ++this.version;
    this.route = route || null;
    this.page = null;
    this.loading = Boolean(route);
    this.failure = "";
    if (!route) return;
    try {
      await route.load();
      if (version !== this.version) return;
      const page = document.createElement(route.tag) as Page;
      page.context = { params: params || {}, path, search: new URLSearchParams(location.search) };
      this.page = page;
      document.title = `${route.title} · ${this.brand.value.site_name}`;
    } catch (cause) {
      if (version === this.version) this.failure = errorMessage(cause);
    } finally {
      if (version === this.version) this.loading = false;
    }
  }
  render() {
    if (this.health.result.isError)
      return html`<gw-auth-layout
          ><gw-error-state .message=${errorMessage(this.health.result.error)}
            ><gw-button variant="outline" @click=${() => this.health.refresh()}
              >Retry connection</gw-button
            ></gw-error-state
          ></gw-auth-layout
        ><gw-toast></gw-toast>`;
    if (!this.health.result.data || (!this.health.result.data.setup && this.auth.value.loading))
      return html`<div class="loading" style="min-height:100svh">
        <gw-spinner></gw-spinner><span>Connecting to your workspace…</span>
      </div>`;
    const content = this.failure
      ? html`<gw-error-state .message=${this.failure}
          ><gw-button
            variant="outline"
            @click=${() => {
              this.key = "";
              void this.resolve();
            }}
            >Reload page</gw-button
          ></gw-error-state
        >`
      : this.loading
        ? html`<div class="loading"><gw-spinner></gw-spinner><span>Loading page…</span></div>`
        : this.page ||
          html`<gw-empty-state heading="Page not found"
            ><gw-button href="/">Go to overview</gw-button></gw-empty-state
          >`;
    const route = this.route,
      project = this.page?.context.params.projectId;
    const workspace = this.teams.value.error
      ? html`<gw-error-state .message=${errorMessage(this.teams.value.error)}
          ><gw-button variant="outline" @click=${() => refreshTeams()}
            >Retry workspace</gw-button
          ></gw-error-state
        >`
      : this.teams.value.loading
        ? html`<div class="loading"><gw-spinner></gw-spinner><span>Loading workspace…</span></div>`
        : project && !this.location.value.pathname.startsWith("/admin")
          ? html`<gw-project-layout .projectId=${project} .current=${this.location.value.pathname}
              >${content}</gw-project-layout
            >`
          : content;
    return html`${
        route?.layout === "auth"
          ? html`<gw-auth-layout>${content}</gw-auth-layout>`
          : route?.layout === "create"
            ? html`<gw-project-create-layout heading=${route.title}
                >${workspace}</gw-project-create-layout
              >`
            : html`<gw-console-layout heading=${route?.title || "Console"}
                >${workspace}</gw-console-layout
              >`
      }<gw-toast></gw-toast>`;
  }
}
define("gw-app", App);
