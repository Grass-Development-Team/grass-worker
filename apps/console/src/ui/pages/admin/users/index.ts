import { QueryController } from "@/infra/query";
import {
  adminApi,
  type AdminBatchItemResult,
  type AdminUserFilters,
  type AdminUserMfaPolicy,
} from "@/services/admin";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing, type PropertyValues } from "lit";
import { Page, badge, field, heading, readForm, secret, table, text } from "../../base";
export class UsersPage extends Page {
  static properties = {
    ...Page.properties,
    filters: { state: true },
    selected: { state: true },
    generated: { state: true },
    results: { state: true },
  };
  filters: AdminUserFilters = {};
  selected = new Set<string>();
  generated = "";
  results: AdminBatchItemResult[] = [];
  private users = new QueryController(this, {
    queryKey: ["admin", "users", this.filters],
    queryFn: () => adminApi.listUsers(this.filters),
  });
  private mfa = new QueryController(this, {
    queryKey: ["admin", "user-mfa", "none"],
    queryFn: () => adminApi.listUserMfa(this.context.params.resourceId),
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues) {
    if (changed.has("context") && this.context.params.resourceId)
      this.mfa.configure({
        queryKey: ["admin", "user-mfa", this.context.params.resourceId],
        queryFn: () => adminApi.listUserMfa(this.context.params.resourceId),
      });
  }
  disconnectedCallback() {
    this.generated = "";
    super.disconnectedCallback();
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.users);
    if (state) return state;
    const users = this.users.result.data!.users,
      creating = this.context.path.endsWith("/new"),
      id = this.context.params.resourceId,
      user = users.find((user) => user.id === id),
      roles = [
        { value: "user", label: "User" },
        { value: "admin", label: "Platform administrator" },
      ];
    if (creating || id) {
      if (id && !user) return html`<gw-alert tone="warning">User not found.</gw-alert>`;
      return html`${heading(creating ? "Create user" : user!.email, "Manage account access and security.", html`<gw-button variant="ghost" href="/admin/users">Back to users</gw-button>`)}${this.error()}${
          this.generated
            ? html`<gw-alert tone="warning"
                >Save this password now. It will not be displayed again.
                <pre>${this.generated}</pre>
                <gw-button
                  variant="outline"
                  @click=${() => this.run(() => navigator.clipboard.writeText(this.generated), undefined, "Password copied.")}
                  >Copy password</gw-button
                ></gw-alert
              >`
            : nothing
        }<gw-card heading="Account"
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () =>
                  creating
                    ? adminApi.createUser({
                        email: text(form, "email"),
                        display_name: text(form, "display_name") || undefined,
                        platform_role: text(form, "platform_role") as "user" | "admin",
                        password: secret(form, "password") || undefined,
                      })
                    : adminApi.updateUser(id, {
                        display_name: text(form, "display_name") || null,
                        platform_role: text(form, "platform_role") as "user" | "admin",
                        status: text(form, "status") as "active" | "disabled",
                      }),
                async (result) => {
                  await this.users.refresh();
                  if (
                    "password" in result &&
                    typeof result.password === "string" &&
                    result.password
                  )
                    this.generated = result.password;
                  else this.go("/admin/users");
                },
              );
            }}
          >
            ${creating ? field({ name: "email", label: "Email", type: "email", required: true }) : nothing}${field({ name: "display_name", label: "Display name" }, user?.display_name)}${field({ name: "platform_role", label: "Platform role", options: roles, required: true }, user?.platform_role || "user")}${
              creating
                ? field({
                    name: "password",
                    label: "Password (optional)",
                    type: "password",
                    description: "Leave blank to generate a password.",
                  })
                : field(
                    {
                      name: "status",
                      label: "Account status",
                      options: [
                        { value: "active", label: "Active" },
                        { value: "disabled", label: "Disabled" },
                      ],
                    },
                    user?.status,
                  )
            }
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}
                >${creating ? "Create user" : "Save user"}</gw-button
              >
            </div>
          </form></gw-card
        >${
          !creating
            ? html`<gw-card heading="Reset password"
                  ><form
                    class="form"
                    @submit=${async (event: Event) => {
                      const form = readForm(event);
                      if (
                        await confirmAction(
                          "Reset user password?",
                          "The user's existing sessions will end.",
                          "Reset password",
                        )
                      )
                        void this.run(
                          () =>
                            adminApi.resetUserPassword(id, secret(form, "password") || undefined),
                          (result) => {
                            this.generated = result.password || "";
                          },
                          "Password reset.",
                        );
                    }}
                  >
                    ${field({ name: "password", label: "New password (optional)", type: "password", description: "Leave blank to generate a password." })}
                    <div class="form-footer">
                      <gw-button type="submit" variant="outline" .loading=${this.busy}
                        >Reset password</gw-button
                      >
                    </div>
                  </form></gw-card
                ><gw-card heading="MFA policy"
                  >${
                    this.queryState(this.mfa) ??
                    html`<form
                        class="form"
                        @submit=${(event: Event) => {
                          const form = readForm(event);
                          void this.run(
                            () =>
                              adminApi.updateUserMfaPolicy(id, {
                                inherit_platform: form.has("inherit_platform"),
                                minimum_factors: Number(text(form, "minimum_factors")),
                                required_factors: [
                                  ...(form.has("require_totp") ? ["totp" as const] : []),
                                  ...(form.has("require_email") ? ["email" as const] : []),
                                ],
                              } satisfies AdminUserMfaPolicy),
                            () => this.mfa.refresh().then(() => undefined),
                          );
                        }}
                      >
                        ${field({ name: "inherit_platform", label: "Inherit platform policy", type: "boolean" }, this.mfa.result.data!.policy.inherit_platform)}${field({ name: "minimum_factors", label: "Minimum factors", type: "number", min: "0", max: "2" }, this.mfa.result.data!.policy.minimum_factors)}${field({ name: "require_totp", label: "Require authenticator app", type: "boolean" }, this.mfa.result.data!.policy.required_factors.includes("totp"))}${field({ name: "require_email", label: "Require email code", type: "boolean" }, this.mfa.result.data!.policy.required_factors.includes("email"))}
                        <div class="form-footer">
                          <gw-button type="submit" .loading=${this.busy}>Save MFA policy</gw-button>
                        </div>
                      </form>
                      ${table(
                        this.mfa.result.data!.factors,
                        [
                          { label: "Method", key: "kind" },
                          { label: "Verified", render: (factor) => badge(factor.verified) },
                          {
                            label: "Actions",
                            render: (factor) =>
                              html`<gw-button
                                variant="ghost"
                                size="sm"
                                @click=${async () => {
                                  if (
                                    await confirmAction(
                                      "Reset verification method?",
                                      "This removes the user's configured factor.",
                                      "Reset factor",
                                    )
                                  )
                                    void this.run(
                                      () => adminApi.resetUserMfaFactor(id, factor.id),
                                      () => this.mfa.refresh().then(() => undefined),
                                      "Factor reset.",
                                    );
                                }}
                                >Reset</gw-button
                              >`,
                          },
                        ],
                        "User MFA factors",
                      )}`
                  }</gw-card
                >`
            : nothing
        }`;
    }
    return html`${heading("Users", "Manage accounts and platform roles.", html`<gw-button href="/admin/users/new">Create user</gw-button>`)}${this.error()}
      <form
        class="grid"
        @submit=${(event: Event) => {
          const form = readForm(event);
          this.filters = {
            q: text(form, "q") || undefined,
            status: (text(form, "status") || undefined) as AdminUserFilters["status"],
            role: (text(form, "role") || undefined) as AdminUserFilters["role"],
          };
          this.selected = new Set();
          this.users.configure({
            queryKey: ["admin", "users", this.filters],
            queryFn: () => adminApi.listUsers(this.filters),
          });
        }}
      >
        ${field({ name: "q", label: "Search", placeholder: "Name or email" })}${field({
          name: "status",
          label: "Status",
          options: [
            { value: "", label: "All statuses" },
            { value: "active", label: "Active" },
            { value: "disabled", label: "Disabled" },
          ],
        })}${field({ name: "role", label: "Role", options: [{ value: "", label: "All roles" }, ...roles] })}
        <div class="form-footer">
          <gw-button type="submit" variant="outline">Apply filters</gw-button>
        </div>
      </form>
      <div class="actions">
        <gw-checkbox
          label="Select all"
          .checked=${users.length > 0 && this.selected.size === users.length}
          @gw-change=${(event: CustomEvent<{ checked: boolean }>) => (this.selected = event.detail.checked ? new Set(users.map((user) => user.id)) : new Set())}
        ></gw-checkbox
        >${["enable", "disable"].map(
          (action) =>
            html`<gw-button
              variant="outline"
              size="sm"
              ?disabled=${!this.selected.size || this.busy}
              @click=${async () => {
                if (
                  action === "enable" ||
                  (await confirmAction(
                    "Disable selected users?",
                    "Selected users will lose account access.",
                    "Disable",
                  ))
                )
                  void this.run(
                    () =>
                      adminApi.batchUsers({
                        action: action as "enable" | "disable",
                        ids: [...this.selected],
                      }),
                    async (result) => {
                      this.results = result.results;
                      this.selected = new Set();
                      await this.users.refresh();
                    },
                    "Batch operation processed.",
                  );
              }}
              >${action}</gw-button
            >`,
        )}
      </div>
      ${table(
        users,
        [
          {
            label: "Select",
            render: (user) =>
              html`<gw-checkbox
                label=${`Select ${user.email}`}
                .checked=${this.selected.has(user.id)}
                @gw-change=${(event: CustomEvent<{ checked: boolean }>) => {
                  const next = new Set(this.selected);
                  event.detail.checked ? next.add(user.id) : next.delete(user.id);
                  this.selected = next;
                }}
              ></gw-checkbox>`,
          },
          {
            label: "Email",
            render: (user) => html`<a href=${`/admin/users/${user.id}`}>${user.email}</a>`,
          },
          { label: "Name", key: "display_name" },
          { label: "Status", render: (user) => badge(user.status) },
          { label: "Role", key: "platform_role" },
        ],
        "Users",
      )}${
        this.results.length
          ? html`<gw-card heading="Batch results"
              >${table(
                this.results,
                [
                  { label: "ID", key: "id" },
                  { label: "Result", render: (item) => badge(item.success ? "success" : "failed") },
                  { label: "Message", key: "message" },
                ],
                "Batch user results",
              )}</gw-card
            >`
          : nothing
      }`;
  }
}
define("gw-page-admin-users", UsersPage);
