import { QueryController, queryClient } from "@/infra/query";
import { setupApi, type SetupStage } from "@/services/setup";
import { define } from "@/ui/components/base";
import "@/ui/widgets/storage-form";
import type { StorageForm } from "@/ui/widgets/storage-form";
import { html } from "lit";
import { Page, field, heading, readForm, secret, text } from "../base";
export class SetupPage extends Page {
  static properties = { ...Page.properties, nodeToken: { state: true } };
  nodeToken = "";
  private setup = new QueryController(this, {
    queryKey: ["setup"],
    queryFn: setupApi.getSetupState,
    retry: false,
    staleTime: 0,
  });
  disconnectedCallback() {
    this.nodeToken = "";
    super.disconnectedCallback();
  }
  private submit(event: Event, stage: SetupStage) {
    const data = readForm(event);
    void this.run(
      async () => {
        if (stage === "database")
          return setupApi.configureDatabase(
            text(data, "host"),
            text(data, "port"),
            text(data, "username"),
            secret(data, "password"),
            text(data, "database"),
          );
        if (stage === "admin")
          return setupApi.createAdmin(
            text(data, "email"),
            secret(data, "password"),
            text(data, "display_name"),
          );
        if (stage === "site")
          return setupApi.configureSite(
            text(data, "name"),
            text(data, "site_url"),
            text(data, "public_base_url"),
          );
        if (stage === "node") {
          const result = await setupApi.createNode(text(data, "name"));
          this.nodeToken = result.token;
          return;
        }
        if (stage === "storage")
          return setupApi.configureStorage(
            this.querySelector<StorageForm>("gw-storage-form")!.input(data),
          );
        if (stage === "finish") {
          await setupApi.finishSetup();
          await queryClient.invalidateQueries({ queryKey: ["health"] });
          this.go("/login", true);
        }
      },
      async () => {
        if (stage !== "node") await this.setup.refresh();
      },
      "",
    );
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.setup);
    if (state) return state;
    const stage = this.setup.result.data!.stage;
    const stages = ["database", "admin", "site", "node", "storage", "finish"],
      index = stages.indexOf(stage);
    return html`${heading("Set up Grass Worker", "A few steps to get your deployment platform ready.")}
      <div class="row">
        ${stages.map((name, i) => html`<gw-badge tone=${i < index ? "success" : i === index ? "info" : "neutral"}>${i + 1}. ${name}</gw-badge>`)}
      </div>
      ${this.error()}<gw-card
        heading=${stage === "finish" ? "Ready to launch" : `Configure ${stage}`}
        >${
          this.nodeToken
            ? html`<div class="stack">
                <gw-alert tone="success"
                  >Your first node is ready. Save this token; it is only shown once.</gw-alert
                ><code class="code break">${this.nodeToken}</code>
                <div class="row">
                  <gw-button
                    variant="outline"
                    @click=${() => this.run(() => navigator.clipboard.writeText(this.nodeToken), undefined, "Node token copied.")}
                    >Copy token</gw-button
                  ><gw-button
                    @click=${() => {
                      this.nodeToken = "";
                      void this.setup.refresh();
                    }}
                    >I've saved the token</gw-button
                  >
                </div>
              </div>`
            : html`<form class="form" @submit=${(event: Event) => this.submit(event, stage)}>
                ${stage === "database" ? html`<div class="grid">${field({ name: "host", label: "Database host", required: true }, "localhost")}${field({ name: "port", label: "Port", type: "number", required: true, min: "1", max: "65535" }, "5432")}${field({ name: "username", label: "Username", required: true })}${field({ name: "password", label: "Password", type: "password" })}${field({ name: "database", label: "Database name", required: true })}</div>` : stage === "admin" ? html`${field({ name: "display_name", label: "Administrator name", required: true })}${field({ name: "email", label: "Email", type: "email", required: true })}${field({ name: "password", label: "Password", type: "password", required: true })}` : stage === "site" ? html`${field({ name: "name", label: "Site name", required: true }, "Grass Worker")}${field({ name: "site_url", label: "Console URL", type: "url", required: true }, window.location.origin)}${field({ name: "public_base_url", label: "Public sites base URL", type: "url", required: true })}` : stage === "node" ? field({ name: "name", label: "Node name", required: true }, "local-node") : stage === "storage" ? html`<gw-storage-form></gw-storage-form>` : stage === "finish" ? html`<p>Database, administrator, site, node, and storage are configured. Finish setup to enable sign-in.</p>` : html`<gw-alert tone="success">Setup is complete.</gw-alert>`}
                <div class="form-footer">
                  ${stage === "complete" ? html`<gw-button href="/login">Sign in</gw-button>` : html`<gw-button type="submit" .loading=${this.busy}>${stage === "finish" ? "Finish setup" : "Continue"}</gw-button>`}
                </div>
              </form>`
        }</gw-card
      >`;
  }
}
define("gw-page-setup", SetupPage);
