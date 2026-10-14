import { QueryController } from "@/infra/query";
import { adminApi } from "@/services/admin";
import { httpsApi, type HttpsSettings } from "@/services/admin/https";
import { regionsApi } from "@/services/regions";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import "@/ui/widgets/storage-form";
import type { StorageForm } from "@/ui/widgets/storage-form";
import { html, nothing } from "lit";
import { Page, details, field, heading, readForm, secret, table, text } from "../../base";
export class InfrastructurePage extends Page {
  static properties = { ...Page.properties, issuer: { state: true }, tested: { state: true } };
  issuer = "";
  tested = "";
  private regions = new QueryController(this, {
    queryKey: ["regions"],
    queryFn: regionsApi.list,
    enabled: window.location.pathname.endsWith("/regions"),
  });
  private https = new QueryController(this, {
    queryKey: ["admin", "https"],
    queryFn: httpsApi.get,
    enabled: window.location.pathname.endsWith("/https"),
  });
  private storage = new QueryController(this, {
    queryKey: ["admin", "storage"],
    queryFn: adminApi.getStorage,
    enabled: window.location.pathname.endsWith("/storage"),
    refetchInterval: 4000,
  });
  render() {
    this.classList.add("page");
    const kind = this.context.params.kind;
    if (kind === "regions")
      return html`${heading("Regions", "Manage the locations available for nodes and deployments.", html`<gw-button variant="ghost" href="/admin/settings/infrastructure">Back to infrastructure</gw-button>`)}${this.error()}<gw-card
          heading="Add region"
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () =>
                  regionsApi.create({
                    code: text(form, "code"),
                    name: text(form, "name") || undefined,
                  }),
                () => this.regions.refresh().then(() => undefined),
                "Region added.",
              );
            }}
          >
            <div class="grid">
              ${field({ name: "code", label: "Code", required: true })}${field({ name: "name", label: "Name" })}
            </div>
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Add region</gw-button>
            </div>
          </form></gw-card
        >${
          this.queryState(this.regions) ??
          table(
            this.regions.result.data!.regions,
            [
              { label: "Code", key: "code" },
              {
                label: "Name",
                render: (region) =>
                  html`<gw-input
                    .value=${region.name || ""}
                    label=${`Rename ${region.code}`}
                    @gw-change=${(event: CustomEvent<{ value: string }>) =>
                      this.run(
                        () => regionsApi.rename(region.code, event.detail.value),
                        () => this.regions.refresh().then(() => undefined),
                      )}
                  ></gw-input>`,
              },
              {
                label: "Actions",
                render: (region) =>
                  html`<gw-button
                    variant="ghost"
                    size="sm"
                    @click=${async () => {
                      if (
                        await confirmAction(
                          "Remove region?",
                          "Regions still used by nodes or domains cannot be removed.",
                          "Remove",
                        )
                      )
                        void this.run(
                          () => regionsApi.remove(region.code),
                          () => this.regions.refresh().then(() => undefined),
                          "Region removed.",
                        );
                    }}
                    >Remove</gw-button
                  >`,
              },
            ],
            "Regions",
          )
        }`;
    if (kind === "https") {
      const state = this.queryState(this.https);
      if (state) return state;
      const data = this.https.result.data!,
        issuer = this.issuer || data.issuer;
      return html`${heading("Custom domain HTTPS", "Choose a certificate authority for automatic issuance and renewal.", html`<gw-button variant="ghost" href="/admin/settings/infrastructure">Back to infrastructure</gw-button>`)}${this.error()}<gw-card
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () =>
                  httpsApi.update({
                    issuer: text(form, "issuer") as HttpsSettings["issuer"],
                    ...(secret(form, "eab_kid") ? { eab_kid: secret(form, "eab_kid") } : {}),
                    ...(secret(form, "eab_hmac_key")
                      ? { eab_hmac_key: secret(form, "eab_hmac_key") }
                      : {}),
                  }),
                async () => {
                  await this.https.refresh();
                  this.querySelector<HTMLFormElement>("form")?.reset();
                },
              );
            }}
          >
            <gw-select
              name="issuer"
              label="Certificate authority"
              .value=${issuer}
              .options=${[
                { value: "letsencrypt", label: "Let's Encrypt" },
                { value: "zerossl", label: "ZeroSSL" },
              ]}
              @gw-change=${(event: CustomEvent<{ value: string }>) => (this.issuer = event.detail.value)}
            ></gw-select
            >${issuer === "zerossl" ? html`${field({ name: "eab_kid", label: "EAB key ID", type: "password", required: !data.zerossl_eab_configured })}${field({ name: "eab_hmac_key", label: "EAB HMAC key", type: "password", required: !data.zerossl_eab_configured, description: data.zerossl_eab_configured ? "Leave both fields blank to preserve configured credentials." : "ZeroSSL requires external account credentials." })}` : nothing}
            <p class="muted">
              New certificates and the next renewal use this authority. HTTP validation requires
              port 80 to reach a regional entry node.
            </p>
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Save HTTPS settings</gw-button>
            </div>
          </form></gw-card
        >`;
    }
    const state = this.queryState(this.storage);
    if (state) return state;
    const data = this.storage.result.data!,
      running = Boolean(data.migration && ["pending", "running"].includes(data.migration.status));
    return html`${heading("Object storage", "Test a target, then migrate existing platform objects.", html`<gw-button variant="ghost" href="/admin/settings/infrastructure">Back to infrastructure</gw-button>`)}${this.error()}${data.maintenance ? html`<gw-alert tone="warning">The platform is in storage maintenance mode.</gw-alert>` : nothing}<gw-card
        heading="Current storage"
        >${details({ backend: data.storage.backend, bucket: data.storage.bucket, endpoint: data.storage.endpoint, local_root: data.storage.local_root, credentials_configured: data.storage.credentials_configured })}</gw-card
      ><gw-card heading="Migration target"
        ><form
          class="form"
          @gw-input=${() => (this.tested = "")}
          @gw-change=${() => (this.tested = "")}
          @submit=${async (event: Event) => {
            const form = readForm(event),
              input = this.querySelector<StorageForm>("gw-storage-form")!.input(form);
            if (this.tested !== JSON.stringify(input)) {
              this.failure = "Test this exact target configuration before migrating.";
              return;
            }
            if (
              await confirmAction(
                "Start storage migration?",
                "The platform will enter maintenance while objects are copied to the target.",
                "Start migration",
              )
            )
              void this.run(
                () => adminApi.createStorageMigration(input),
                () => this.storage.refresh().then(() => undefined),
                "Storage migration started.",
              );
          }}
        >
          <gw-storage-form .configuration=${data.storage}></gw-storage-form>
          <div class="form-footer">
            <gw-button
              variant="outline"
              ?disabled=${this.busy || running}
              @click=${() => {
                const form = this.querySelector<HTMLFormElement>("form")!;
                if (!form.reportValidity()) return;
                const input = this.querySelector<StorageForm>("gw-storage-form")!.input(
                  new FormData(form),
                );
                void this.run(
                  () => adminApi.testStorage(input),
                  () => {
                    this.tested = JSON.stringify(input);
                  },
                  "Storage target tested successfully.",
                );
              }}
              >Test target</gw-button
            ><gw-button
              type="submit"
              variant="primary"
              ?disabled=${this.busy || running || !this.tested}
              >Start migration</gw-button
            >
          </div>
        </form></gw-card
      >${data.migration ? html`<gw-card heading="Migration progress">${details({ status: data.migration.status, copied_objects: data.migration.copied_objects, total_objects: data.migration.total_objects, copied_bytes: data.migration.copied_bytes, total_bytes: data.migration.total_bytes, error: data.migration.last_error })}</gw-card>` : nothing}`;
  }
}
define("gw-page-admin-infrastructure", InfrastructurePage);
