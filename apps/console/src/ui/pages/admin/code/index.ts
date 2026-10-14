import { QueryController } from "@/infra/query";
import { adminApi, type AdminCodeStatus, type AdminGeneratedCode } from "@/services/admin";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing } from "lit";
import { Page, badge, field, heading, readForm, table, text } from "../../base";
export class CodesPage extends Page {
  static properties = {
    ...Page.properties,
    page: { state: true },
    scope: { state: true },
    status: { state: true },
    generated: { state: true },
  };
  page = 1;
  scope = "";
  status = "";
  generated: AdminGeneratedCode[] = [];
  private codes = new QueryController(this, {
    queryKey: ["admin", "codes", 1],
    queryFn: () => adminApi.listCodes(),
  });
  private refresh() {
    this.codes.configure({
      queryKey: ["admin", "codes", this.page, this.scope, this.status],
      queryFn: () =>
        adminApi.listCodes({
          page: this.page,
          scope: this.scope || undefined,
          status: (this.status || undefined) as AdminCodeStatus | undefined,
        }),
    });
  }
  render() {
    this.classList.add("page");
    const data = this.codes.result.data,
      creating = this.context.path.endsWith("/new");
    if (creating)
      return html`${heading("Generate codes", "Create registration or other supported scoped codes.", html`<gw-button variant="ghost" href="/admin/code">Back to codes</gw-button>`)}${this.error()}<gw-card
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () =>
                  adminApi.generateCodes({
                    scope: text(form, "scope"),
                    count: Number(text(form, "count")),
                    never_expires: form.has("never_expires"),
                    expires_in_days: form.has("never_expires")
                      ? null
                      : Number(text(form, "expires_in_days")),
                  }),
                (result) => {
                  this.generated = result.codes;
                },
                "Codes generated.",
              );
            }}
          >
            ${field({ name: "scope", label: "Scope", required: true, options: (data?.scopes || ["registration"]).map((value) => ({ value, label: value })) }, data?.scopes[0] || "registration")}${field({ name: "count", label: "Count", type: "number", required: true, min: "1", max: "1000" }, 10)}${field({ name: "expires_in_days", label: "Expires in (days)", type: "number", min: "1" }, 7)}${field({ name: "never_expires", label: "Never expire", type: "boolean" }, false)}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Generate codes</gw-button>
            </div>
          </form></gw-card
        >${
          this.generated.length
            ? html`<gw-card heading="Generated codes"
                ><div class="stack">
                  <pre class="code">${this.generated.map((code) => code.code).join("\n")}</pre>
                  <gw-button
                    variant="outline"
                    @click=${() => this.run(() => navigator.clipboard.writeText(this.generated.map((code) => code.code).join("\n")), undefined, "Codes copied.")}
                    >Copy all codes</gw-button
                  >
                </div></gw-card
              >`
            : nothing
        }`;
    return html`${heading("Scoped codes", "Track available, used, expired, and revoked codes.", html`<gw-button href="/admin/code/new">Generate codes</gw-button>`)}${this.error()}
      <div class="grid">
        <gw-select
          label="Scope filter"
          .value=${this.scope}
          .options=${[{ value: "", label: "All scopes" }, ...(data?.scopes || []).map((value) => ({ value, label: value }))]}
          @gw-change=${(event: CustomEvent<{ value: string }>) => {
            this.scope = event.detail.value;
            this.page = 1;
            this.refresh();
          }}
        ></gw-select
        ><gw-select
          label="Status filter"
          .value=${this.status}
          .options=${[{ value: "", label: "All statuses" }, ...["available", "used", "expired", "revoked"].map((value) => ({ value, label: value }))]}
          @gw-change=${(event: CustomEvent<{ value: string }>) => {
            this.status = event.detail.value;
            this.page = 1;
            this.refresh();
          }}
        ></gw-select>
      </div>
      ${
        this.queryState(this.codes) ??
        html`${table(
            data!.codes,
            [
              { label: "Code", render: (code) => html`<code>${code.code}</code>` },
              { label: "Scope", key: "scope" },
              { label: "Status", render: (code) => badge(code.status) },
              { label: "Expires", render: (code) => code.expires_at || "Never" },
              { label: "Used by", render: (code) => code.used_by?.email || "—" },
              {
                label: "Actions",
                render: (code) =>
                  code.status === "available"
                    ? html`<gw-button
                        variant="ghost"
                        size="sm"
                        @click=${async () => {
                          if (
                            await confirmAction(
                              "Revoke code?",
                              "This code can no longer be redeemed.",
                              "Revoke",
                            )
                          )
                            void this.run(
                              () => adminApi.revokeCode(code.id),
                              () => this.codes.refresh().then(() => undefined),
                              "Code revoked.",
                            );
                        }}
                        >Revoke</gw-button
                      >`
                    : nothing,
              },
            ],
            "Scoped codes",
          )}<gw-pagination
            .page=${this.page}
            .pages=${data!.pagination.total_pages}
            @gw-page=${(event: CustomEvent<{ page: number }>) => {
              this.page = event.detail.page;
              this.refresh();
            }}
          ></gw-pagination>`
      }`;
  }
}
define("gw-page-admin-codes", CodesPage);
