import { QueryController } from "@/infra/query";
import { teamsApi } from "@/services/teams";
import { teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing } from "lit";
import { Page, badge, field, heading, readForm, secret, table, text } from "../../base";
export class SourceAccessPage extends Page {
  static properties = { ...Page.properties, kind: { state: true } };
  kind = "https";
  private team = teams.value.active;
  private credentials = new QueryController(this, {
    queryKey: ["team", this.team?.id, "credentials"],
    queryFn: () => teamsApi.listSourceCredentials(this.team!.id),
  });
  private keys = new QueryController(this, {
    queryKey: ["team", this.team?.id, "ssh-keys"],
    queryFn: () => teamsApi.listSshHostKeys(this.team!.id),
  });
  render() {
    this.classList.add("page");
    const state = this.queryState(this.credentials);
    if (state) return state;
    const credentials = this.credentials.result.data!.credentials,
      creating = this.context.path.endsWith("/new"),
      id = this.context.params.credentialId,
      editing = credentials.find((item) => item.id === id),
      kind = editing?.kind || this.kind;
    if (creating || id)
      return html`${heading(editing ? "Rotate Git credential" : "Add Git credential", "Secrets are encrypted and write-only. Queued deployments keep their fixed credential version.", html`<gw-button variant="ghost" href="/settings/team/source-access">Back to source access</gw-button>`)}${this.error()}<gw-card
          ><form
            class="form"
            @submit=${(event: Event) => {
              const data = readForm(event),
                input = {
                  username: text(data, "username"),
                  ...(kind === "https"
                    ? { secret: secret(data, "secret") }
                    : {
                        private_key: secret(data, "private_key"),
                        passphrase: secret(data, "passphrase") || undefined,
                      }),
                };
              void this.run(
                () =>
                  editing
                    ? teamsApi.rotateSourceCredential(this.team!.id, editing.id, input)
                    : teamsApi.createSourceCredential(this.team!.id, {
                        ...input,
                        name: text(data, "name"),
                        repository_url: text(data, "repository_url"),
                      }),
                async () => {
                  await this.credentials.refresh();
                  this.go("/settings/team/source-access");
                },
                "Credential saved.",
              );
            }}
          >
            ${
              !editing
                ? html`<gw-select
                      label="Credential type"
                      .value=${kind}
                      .options=${[
                        { value: "https", label: "HTTPS token" },
                        { value: "ssh", label: "SSH key" },
                      ]}
                      @gw-change=${(event: CustomEvent<{ value: string }>) => (this.kind = event.detail.value)}
                    ></gw-select
                    >${field({ name: "name", label: "Credential name", required: true })}${field({ name: "repository_url", label: "Repository URL", required: true, placeholder: kind === "https" ? "https://github.com/acme/site.git" : "git@github.com:acme/site.git" })}`
                : html`<p>${editing.name} · ${editing.host}:${editing.port}</p>`
            }${field({ name: "username", label: "Username", required: true }, editing?.username || "")}${kind === "https" ? field({ name: "secret", label: "Token", type: "password", required: true }) : html`${field({ name: "private_key", label: "Private key PEM", type: "textarea", required: true })}${field({ name: "passphrase", label: "Passphrase", type: "password" })}`}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}
                >${editing ? "Rotate credential" : "Add credential"}</gw-button
              >
            </div>
          </form></gw-card
        >`;
    return html`${heading("Git source access", "Private repository credentials and trusted SSH host keys.", html`<gw-button href="/settings/team/source-access/new">Add credential</gw-button>`)}${this.error()}${table(
        credentials,
        [
          { label: "Name", key: "name" },
          { label: "Host", render: (item) => `${item.host}:${item.port}` },
          { label: "Kind", key: "kind" },
          { label: "Status", render: (item) => badge(item.revoked_at ? "revoked" : "active") },
          {
            label: "Actions",
            render: (item) =>
              !item.revoked_at
                ? html`<div class="actions">
                    <gw-button
                      variant="ghost"
                      size="sm"
                      href=${`/settings/team/source-access/${item.id}`}
                      >Rotate</gw-button
                    ><gw-button
                      variant="ghost"
                      size="sm"
                      @click=${async () => {
                        if (
                          await confirmAction(
                            "Revoke credential?",
                            "New deployments will no longer be able to use this credential.",
                            "Revoke",
                          )
                        )
                          void this.run(
                            () => teamsApi.revokeSourceCredential(this.team!.id, item.id),
                            () => this.credentials.refresh().then(() => undefined),
                            "Credential revoked.",
                          );
                      }}
                      >Revoke</gw-button
                    >
                  </div>`
                : nothing,
          },
        ],
        "Git credentials",
      )}<gw-card
        heading="SSH host keys"
        description="Check the fingerprint through a trusted channel before approving a key."
        >${
          this.queryState(this.keys) ??
          table(
            this.keys.result.data!.host_keys,
            [
              { label: "Host", render: (key) => `${key.host}:${key.port}` },
              {
                label: "Fingerprint",
                render: (key) => html`<code class="break">${key.fingerprint_sha256}</code>`,
              },
              { label: "Status", render: (key) => badge(key.status) },
              {
                label: "Actions",
                render: (key) =>
                  key.status === "pending"
                    ? html`<div class="actions">
                        <gw-button
                          variant="outline"
                          size="sm"
                          @click=${async () => {
                            if (
                              await confirmAction(
                                "Trust SSH host key?",
                                `Verify this fingerprint before continuing: ${key.fingerprint_sha256}`,
                                "Trust key",
                              )
                            )
                              void this.run(
                                () => teamsApi.approveSshHostKey(this.team!.id, key.id),
                                () => this.keys.refresh().then(() => undefined),
                              );
                          }}
                          >Approve</gw-button
                        ><gw-button
                          variant="ghost"
                          size="sm"
                          @click=${() =>
                            this.run(
                              () => teamsApi.rejectSshHostKey(this.team!.id, key.id),
                              () => this.keys.refresh().then(() => undefined),
                            )}
                          >Reject</gw-button
                        >
                      </div>`
                    : nothing,
              },
            ],
            "SSH host keys",
          )
        }</gw-card
      >`;
  }
}
define("gw-page-source-access", SourceAccessPage);
