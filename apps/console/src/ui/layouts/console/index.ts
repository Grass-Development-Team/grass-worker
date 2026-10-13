import { StoreController } from "@/infra/controllers/store";
import { location, navigate } from "@/infra/router";
import { errorMessage } from "@/infra/serializer";
import { changeTheme, theme } from "@/infra/theme";
import { authApi } from "@/services/auth";
import { session, sessionContext } from "@/services/auth/state";
import { branding } from "@/services/branding/state";
import { canViewTeamAudit } from "@/services/teams/permissions";
import { teams } from "@/services/teams/state";
import { GwElement, componentStyles, define } from "@/ui/components/base";
import { notify } from "@/ui/components/toast";
import "@/ui/widgets/notification-center";
import "@/ui/widgets/team-switcher";
import { ContextConsumer } from "@lit/context";
import { html, nothing } from "lit";
import styles from "./style.css?inline";
export class ConsoleLayout extends GwElement {
  static properties = { heading: {}, open: { state: true }, collapsed: { state: true } };
  static styles = componentStyles(styles);
  heading = "";
  open = false;
  collapsed = false;
  private theme = new StoreController(this, theme);
  private auth = new StoreController(this, session);
  private teams = new StoreController(this, teams);
  private brand = new StoreController(this, branding);
  private location = new StoreController(this, location);
  private context = new ContextConsumer(this, { context: sessionContext, subscribe: true });
  protected updated(changed: Map<string, unknown>) {
    if (changed.has("heading")) this.open = false;
  }
  private nav() {
    const path = this.location.value.pathname;
    const admin = path.startsWith("/admin"),
      account = path.startsWith("/account");
    const items = admin
      ? [
          ["Reviews", "/admin/reviews", "shield"],
          ["Projects", "/admin/projects", "folder"],
          ["Nodes", "/admin/nodes", "activity"],
          ["Host sources", "/admin/host-sources", "globe"],
          ["Regional ingresses", "/admin/regional-ingresses", "globe"],
          ["Quota plans", "/admin/quota-plans", "activity"],
          ["Team groups", "/admin/team-groups", "user"],
          ["Users", "/admin/users", "user"],
          ["Teams", "/admin/teams", "user"],
          ["Registration", "/admin/registration", "mail"],
          ["Codes", "/admin/code", "key"],
          ["Basic settings", "/admin/settings/basic", "settings"],
          ["Announcements", "/admin/settings/announcements", "bell"],
          ["Email delivery", "/admin/settings/email", "mail"],
          ["Authentication", "/admin/settings/authentication", "shield"],
          ["Governance", "/admin/settings/governance", "shield"],
          ["Infrastructure", "/admin/settings/infrastructure", "globe"],
          ["Runtime", "/admin/settings/runtime", "terminal"],
          ["Audit", "/admin/audit", "activity"],
          ["Cleanup", "/admin/cleanup", "trash"],
        ]
      : account
        ? [
            ["Profile", "/account/profile", "user"],
            ["Security", "/account/security", "shield"],
          ]
        : [
            ["Overview", "/", "home"],
            ["Projects", "/projects", "folder"],
            ["Usage", "/quota", "activity"],
            ["Team settings", "/settings/team", "settings"],
            ["Members", "/settings/members", "user"],
            ...(this.teams.value.role && canViewTeamAudit(this.teams.value.role)
              ? [["Audit", "/settings/audit", "activity"]]
              : []),
          ];
    return html`<gw-sidebar label=${admin ? "Administration" : account ? "Account" : "Workspace"}
      >${items.map(([label, href, icon]) => html`<a href=${href} aria-label=${label} aria-current=${path === href || (href !== "/" && path.startsWith(`${href}/`)) ? "page" : "false"}><gw-icon name=${icon}></gw-icon><span>${label}</span></a>`)}</gw-sidebar
    >`;
  }
  private async signOut(): Promise<void> {
    try {
      await authApi.logout();
      window.dispatchEvent(new Event("grass-worker:api-unauthorized"));
      navigate("/login");
    } catch (cause) {
      notify(errorMessage(cause), "error");
    }
  }
  protected accountMenu(compact = false) {
    const user = this.context.value?.value.user ?? this.auth.value.user;
    return html`<gw-dropdown-menu label="Account menu"
      ><span slot="trigger" class=${compact ? "account compact" : "account"}
        ><gw-avatar
          src=${user?.avatar_url || ""}
          alt=${user?.display_name || user?.email || "Account"}
        ></gw-avatar
        >${compact ? nothing : html`<span>${user?.display_name || user?.email}</span>`}</span
      ><a href="/account/profile" role="menuitem">Personal settings</a
      ><a href="/account/security" role="menuitem">Security</a
      >${user?.platform_role === "admin" ? html`<a href="/admin/reviews" role="menuitem">Administration</a>` : nothing}<button
        role="menuitem"
        @click=${() => this.signOut()}
      >
        Sign out
      </button></gw-dropdown-menu
    >`;
  }
  protected headerActions() {
    return html`<div class="header-actions">
      <gw-notification-center></gw-notification-center
      ><gw-button
        variant="ghost"
        size="icon"
        aria-label="Toggle theme"
        @click=${(event: MouseEvent) => {
          const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
          void changeTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark", {
            x: event.clientX || rect.left + rect.width / 2,
            y: event.clientY || rect.top + rect.height / 2,
          });
        }}
        ><gw-icon
          name=${document.documentElement.dataset.theme === "dark" ? "moon" : "sun"}
        ></gw-icon></gw-button
      ><gw-dropdown-menu label="Theme preference"
        ><span slot="trigger"><gw-icon name="down"></gw-icon></span
        >${["light", "dark", "system"].map((value) => html`<button role="menuitem" @click=${(event: MouseEvent) => changeTheme(value as "light" | "dark" | "system", { x: event.clientX, y: event.clientY })}>${value[0].toUpperCase() + value.slice(1)}</button>`)}</gw-dropdown-menu
      >
    </div>`;
  }
  render() {
    const brand = this.brand.value;
    return html`<div
      class=${`shell ${this.open ? "open" : ""} ${this.collapsed ? "collapsed" : ""}`}
    >
      ${this.open ? html`<button class="backdrop" aria-label="Close navigation" @click=${() => (this.open = false)}></button>` : nothing}
      <aside>
        <a href="/" class="brand"
          >${brand.logo_url ? html`<gw-image src=${brand.logo_url} alt="" style="width:24px;height:24px"><gw-icon slot="fallback" name="activity"></gw-icon></gw-image>` : html`<gw-icon name="activity"></gw-icon>`}<strong
            >${brand.site_name}</strong
          ></a
        >
        <div class="workspace">
          <gw-team-switcher></gw-team-switcher><a class="new-team" href="/teams/new">Create team</a>
        </div>
        <div class="navigation">${this.nav()}</div>
        <div class="aside-footer">
          ${this.accountMenu()}<small>Grass Worker · v${brand.version}</small>
        </div>
      </aside>
      <div class="main">
        <header>
          <gw-button
            variant="ghost"
            size="icon"
            aria-label="Toggle navigation"
            @click=${() => {
              if (matchMedia("(max-width: 900px)").matches) this.open = !this.open;
              else this.collapsed = !this.collapsed;
            }}
            ><gw-icon name="menu"></gw-icon></gw-button
          ><gw-breadcrumb
            ><a href="/">${this.teams.value.active?.name || "Workspace"}</a><span>/</span
            ><span>${this.heading}</span></gw-breadcrumb
          >
          ${this.headerActions()}
        </header>
        <main><slot></slot></main>
      </div>
    </div>`;
  }
}
define("gw-console-layout", ConsoleLayout);
