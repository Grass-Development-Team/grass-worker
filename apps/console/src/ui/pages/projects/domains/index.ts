import { QueryController } from "@/infra/query";
import { projectsApi } from "@/services/projects";
import { regionsApi } from "@/services/regions";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing, type PropertyValues } from "lit";
import { badge, details, field, heading, readForm, secret, table, text } from "../../base";
import { ProjectPage } from "../shared";
export class DomainsPage extends ProjectPage {
  private hosts = new QueryController(this, {
    queryKey: ["project-hosts", "none"],
    queryFn: () => projectsApi.listHosts(this.projectId),
    enabled: false,
  });
  private regions = new QueryController(this, { queryKey: ["regions"], queryFn: regionsApi.list });
  protected willUpdate(changed: PropertyValues) {
    super.willUpdate(changed);
    if (changed.has("context"))
      this.hosts.configure({
        queryKey: ["project", this.projectId, "hosts"],
        queryFn: () => projectsApi.listHosts(this.projectId),
        enabled: Boolean(this.projectId),
        refetchInterval: 10000,
      });
  }
  private refresh() {
    return this.hosts.refresh().then(() => undefined);
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.project) ?? this.queryState(this.hosts);
    if (state) return state;
    const hosts = this.hosts.result.data!.hosts,
      creating = this.context.path.endsWith("/new"),
      hostId = this.context.params.hostId,
      host = hosts.find((host) => host.id === hostId),
      importing = this.context.path.endsWith("/import");
    if (creating)
      return html`${heading("Add domain", "Connect a custom domain to this project.", html`<gw-button variant="ghost" href=${`/projects/${this.projectId}/domains`}>Back to domains</gw-button>`)}${this.error()}${
        this.canEdit
          ? html`<gw-card
              ><form
                class="form"
                @submit=${(event: Event) => {
                  const form = readForm(event);
                  void this.run(
                    () =>
                      projectsApi.createHost(this.projectId, {
                        host: text(form, "host"),
                        environment: text(form, "environment") as "production" | "preview" | "all",
                        region: text(form, "region") || undefined,
                      }),
                    async () => {
                      await this.refresh();
                      this.go(`/projects/${this.projectId}/domains`);
                    },
                    "Domain added.",
                  );
                }}
              >
                ${field({ name: "host", label: "Domain", required: true, placeholder: "app.example.com", description: "Enter a hostname without a scheme or path." })}${field(
                  {
                    name: "environment",
                    label: "Environment",
                    options: [
                      { value: "production", label: "Production" },
                      { value: "preview", label: "Preview" },
                      { value: "all", label: "All environments" },
                    ],
                  },
                  "production",
                )}${this.queryState(this.regions) ?? field({ name: "region", label: "Region", required: true, options: [{ value: "", label: "Choose a region" }, ...this.regions.result.data!.regions.map((region) => ({ value: region.code, label: region.name || region.code }))] })}
                <div class="form-footer">
                  <gw-button type="submit" .loading=${this.busy}>Add domain</gw-button>
                </div>
              </form></gw-card
            >`
          : html`<gw-alert tone="warning">You do not have permission to add domains.</gw-alert>`
      }`;
    if (hostId) {
      if (!host) return html`<gw-alert tone="warning">Domain not found.</gw-alert>`;
      if (importing)
        return html`${heading("Import certificate", host.host, html`<gw-button variant="ghost" href=${`/projects/${this.projectId}/domains/${host.id}`}>Back to domain</gw-button>`)}${this.error()}${
          this.canEdit && host.ownership_status === "verified"
            ? html`<gw-card heading="Certificate and private key"
                ><form
                  class="form"
                  @submit=${(event: Event) => {
                    const form = readForm(event);
                    void this.run(
                      () =>
                        projectsApi.importHostCertificate(this.projectId, host.id, {
                          certificate_pem: secret(form, "certificate_pem"),
                          private_key_pem: secret(form, "private_key_pem"),
                        }),
                      async () => {
                        await this.refresh();
                        this.go(`/projects/${this.projectId}/domains/${host.id}`);
                      },
                      "Certificate imported.",
                    );
                  }}
                >
                  ${field({ name: "certificate_pem", label: "Certificate PEM", type: "textarea", required: true })}${field({ name: "private_key_pem", label: "Private key PEM", type: "textarea", required: true })}
                  <div class="form-footer">
                    <gw-button type="submit" .loading=${this.busy}>Import certificate</gw-button>
                  </div>
                </form></gw-card
              >`
            : html`<gw-alert tone="warning"
                >Verify domain ownership before importing a certificate.</gw-alert
              >`
        }`;
      const certificate = host.certificate;
      return html`${heading(host.host, "DNS, ownership, connection, and HTTPS settings.", html`<gw-button variant="ghost" href=${`/projects/${this.projectId}/domains`}>Back to domains</gw-button>`)}${this.error()}
        <div class="row">
          ${badge(host.connection_state || host.status)}${badge(host.ownership_status)}${certificate ? badge(certificate.https_ready ? "HTTPS ready" : certificate.status) : nothing}
        </div>
        ${host.failure_reason || host.ownership_error || host.onboarding?.dns_error ? html`<gw-alert tone="error">${host.failure_reason || host.ownership_error || host.onboarding?.dns_error}</gw-alert>` : nothing}<gw-card
          heading="DNS records"
          description="Add these records at your DNS provider, then check the connection."
          >${
            host.ingress
              ? html`${table(
                    [host.ingress.cname, host.ingress.txt],
                    [
                      { label: "Type", key: "record_type" },
                      { label: "Name", key: "name" },
                      {
                        label: "Value",
                        render: (record) => ("target" in record ? record.target : record.value),
                      },
                    ],
                    "Required DNS records",
                  )}
                  <p class="muted" style="margin-top:16px">
                    HTTPS validation requires public port 80 to reach an entry node.
                  </p>`
              : html`<p>DNS instructions are not available yet.</p>`
          }${
            this.canEdit
              ? html`<div class="row">
                  <gw-button
                    variant="outline"
                    .loading=${this.busy}
                    @click=${() =>
                      this.run(
                        () => projectsApi.verifyHost(this.projectId, host.id),
                        () => this.refresh(),
                        "Connection checked.",
                      )}
                    >Check connection</gw-button
                  >${
                    host.kind === "platform"
                      ? html`<gw-button
                          variant="outline"
                          ?disabled=${this.busy}
                          @click=${() =>
                            this.run(
                              () => projectsApi.provisionHost(this.projectId, host.id),
                              () => this.refresh(),
                              "Provisioning requested.",
                            )}
                          >Retry provisioning</gw-button
                        >`
                      : nothing
                  }
                </div>`
              : nothing
          }</gw-card
        ><gw-card heading="HTTPS certificate"
          >${
            certificate
              ? html`<div class="stack">
                  ${details({ issuer: certificate.issuer, status: certificate.status, https_ready: certificate.https_ready, installed_nodes: certificate.installed_nodes, required_nodes: certificate.required_nodes, issued: certificate.issued_at, expires: certificate.expires_at, next_attempt: certificate.retry_at })}${certificate.error ? html`<gw-alert tone="error">${certificate.error}</gw-alert>` : nothing}${
                    this.canEdit && host.kind === "custom"
                      ? html`${
                          certificate.issuer !== "manual"
                            ? html`<gw-switch
                                label="Automatic renewal"
                                .checked=${certificate.auto_renew}
                                ?disabled=${this.busy || certificate.status === "issuing"}
                                @gw-change=${(event: CustomEvent<{ checked: boolean }>) =>
                                  this.run(
                                    () =>
                                      projectsApi.updateHostCertificate(this.projectId, host.id, {
                                        certificate_auto_renew: event.detail.checked,
                                      }),
                                    () => this.refresh(),
                                  )}
                              ></gw-switch>`
                            : nothing
                        }${
                          host.ownership_status === "verified"
                            ? html`<div class="row">
                                ${
                                  certificate.issuer === "manual"
                                    ? html`<gw-button
                                        variant="outline"
                                        @click=${() =>
                                          this.run(
                                            () =>
                                              projectsApi.updateHostCertificate(
                                                this.projectId,
                                                host.id,
                                                {
                                                  certificate_issuer: certificate.platform_issuer,
                                                  certificate_auto_renew: true,
                                                },
                                              ),
                                            () => this.refresh(),
                                          )}
                                        >Use managed certificate</gw-button
                                      >`
                                    : html`<gw-button
                                        variant="outline"
                                        ?disabled=${this.busy || certificate.status === "issuing"}
                                        @click=${() =>
                                          this.run(
                                            () =>
                                              projectsApi.renewHostCertificate(
                                                this.projectId,
                                                host.id,
                                              ),
                                            () => this.refresh(),
                                            "Renewal requested.",
                                          )}
                                        >Renew certificate</gw-button
                                      >`
                                }<gw-button
                                  variant="outline"
                                  href=${`/projects/${this.projectId}/domains/${host.id}/certificate/import`}
                                  >Import certificate</gw-button
                                >
                              </div>`
                            : nothing
                        }`
                      : nothing
                  }
                </div>`
              : html`<p>Certificates are issued after DNS and ownership verification.</p>`
          }</gw-card
        ><gw-card heading="Domain details"
          >${details({ kind: host.kind, environment: host.environment, region: host.region, primary: host.is_primary, status: host.status })}</gw-card
        >${
          host.provision_events?.length
            ? html`<gw-card heading="Provisioning activity"
                >${table(
                  host.provision_events,
                  [
                    { label: "Operation", key: "operation" },
                    { label: "Status", render: (event) => badge(event.status) },
                    { label: "Error", key: "error_message" },
                    { label: "Time", key: "created_at" },
                  ],
                  "Provisioning activity",
                )}</gw-card
              >`
            : nothing
        }`;
    }
    return html`${heading("Domains", "Connect your project to the web.", this.canEdit ? html`<gw-button href=${`/projects/${this.projectId}/domains/new`}>Add domain</gw-button>` : nothing)}${this.error()}${table(
      hosts,
      [
        {
          label: "Domain",
          render: (host) =>
            html`<a href=${`/projects/${this.projectId}/domains/${host.id}`}>${host.host}</a
              >${host.is_primary ? html` <gw-badge>Primary</gw-badge>` : nothing}`,
        },
        { label: "Environment", key: "environment" },
        { label: "Connection", render: (host) => badge(host.connection_state || host.status) },
        {
          label: "HTTPS",
          render: (host) =>
            badge(host.certificate?.https_ready ? "ready" : host.certificate?.status || "pending"),
        },
        {
          label: "Actions",
          render: (host) =>
            html`<div class="actions">
              <gw-button
                variant="ghost"
                size="sm"
                href=${`/projects/${this.projectId}/domains/${host.id}`}
                >Settings</gw-button
              >${
                this.canEdit && !host.is_primary
                  ? html`<gw-button
                      variant="ghost"
                      size="sm"
                      @click=${() =>
                        this.run(
                          () => projectsApi.setPrimaryHost(this.projectId, host.id),
                          () => this.refresh(),
                          "Primary domain updated.",
                        )}
                      >Make primary</gw-button
                    >`
                  : nothing
              }${
                this.canEdit
                  ? html`<gw-button
                      variant="ghost"
                      size="sm"
                      @click=${async () => {
                        if (
                          await confirmAction(
                            "Remove domain?",
                            `${host.host} will no longer point to this project.`,
                            "Remove",
                          )
                        )
                          void this.run(
                            () => projectsApi.removeHost(this.projectId, host.id),
                            () => this.refresh(),
                            "Domain removed.",
                          );
                      }}
                      >Remove</gw-button
                    >`
                  : nothing
              }
            </div>`,
        },
      ],
      "Project domains",
    )}`;
  }
}
define("gw-page-domains", DomainsPage);
