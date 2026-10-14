import { QueryController } from "@/infra/query";
import { adminApi, type NodeConfiguration } from "@/services/admin";
import { expandConfiguration, flattenConfiguration } from "@/services/admin/configuration";
import { regionsApi } from "@/services/regions";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing, type PropertyValues } from "lit";
import {
  Page,
  badge,
  details,
  field,
  formValues,
  heading,
  readForm,
  table,
  text,
  type FieldSpec,
} from "../../base";
export class NodesPage extends Page {
  static properties = { ...Page.properties, token: { state: true }, warnings: { state: true } };
  token = "";
  warnings: string[] = [];
  private nodes = new QueryController(this, {
    queryKey: ["admin", "nodes"],
    queryFn: adminApi.listNodes,
    refetchInterval: 4000,
  });
  private regions = new QueryController(this, { queryKey: ["regions"], queryFn: regionsApi.list });
  private deletion = new QueryController(this, {
    queryKey: ["admin", "node-deletion", "none"],
    queryFn: () => adminApi.nodeDeletionPlan(this.context.params.resourceId),
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues) {
    if (changed.has("context") && this.context.path.endsWith("/delete"))
      this.deletion.configure({
        queryKey: ["admin", "node-deletion", this.context.params.resourceId],
        queryFn: () => adminApi.nodeDeletionPlan(this.context.params.resourceId),
      });
  }
  disconnectedCallback() {
    this.token = "";
    super.disconnectedCallback();
  }
  private refresh() {
    return this.nodes.refresh().then(() => undefined);
  }
  private tokenPanel() {
    return this.token
      ? html`<gw-alert tone="warning"
          >Save this token now; it is shown only once.
          <pre class="break">${this.token}</pre>
          <gw-button
            variant="outline"
            @click=${() => this.run(() => navigator.clipboard.writeText(this.token), undefined, "Node token copied.")}
            >Copy token</gw-button
          ></gw-alert
        >`
      : nothing;
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.nodes);
    if (state) return state;
    const data = this.nodes.result.data!,
      creating = this.context.path.endsWith("/new"),
      id = this.context.params.resourceId,
      node = data.nodes.find((node) => node.id === id);
    if (creating)
      return html`${heading("Create node", "Register a build or serve node.", html`<gw-button variant="ghost" href="/admin/nodes">Back to nodes</gw-button>`)}${this.error()}${this.tokenPanel()}${this.warnings.map((warning) => html`<gw-alert tone="warning">${warning}</gw-alert>`)}<gw-card
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () =>
                  adminApi.createNode({
                    name: text(form, "name"),
                    region: text(form, "region") || undefined,
                    start_local: form.has("start_local"),
                  }),
                async (result) => {
                  this.token = result.token;
                  this.warnings = result.warnings;
                  await this.refresh();
                },
                "Node registered.",
              );
            }}
          >
            ${field({ name: "name", label: "Node name", required: true })}${field({ name: "region", label: "Region", required: true, options: [{ value: "", label: "Choose region" }, ...(this.regions.result.data?.regions || []).map((region) => ({ value: region.code, label: region.name || region.code }))] })}${field({ name: "start_local", label: "Start managed local process", type: "boolean" }, false)}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Create node</gw-button>
            </div>
          </form></gw-card
        >`;
    if (id) {
      if (!node) return html`<gw-alert tone="warning">Node not found.</gw-alert>`;
      if (this.context.path.endsWith("/delete")) {
        const loading = this.queryState(this.deletion);
        if (loading) return loading;
        const plan = this.deletion.result.data!;
        return html`${heading("Delete node", node.name, html`<gw-button variant="ghost" href=${`/admin/nodes/${id}`}>Back to node</gw-button>`)}${this.error()}<gw-card
            >${details({ assigned_deployments: plan.assigned_deployments, active_builds: plan.active_builds, requires_migration_target: plan.requires_target })}
            <form
              class="form"
              style="margin-top:24px"
              @submit=${async (event: Event) => {
                const form = readForm(event);
                if (
                  await confirmAction(
                    "Queue node deletion?",
                    "Services will migrate to the chosen node before this node is removed.",
                    "Queue deletion",
                  )
                )
                  void this.run(
                    () =>
                      adminApi.queueNodeDeletion(id, {
                        target_node_id: text(form, "target_node_id") || null,
                      }),
                    () => this.go("/admin/nodes"),
                    "Node deletion queued.",
                  );
              }}
            >
              ${field({ name: "target_node_id", label: "Migration target", required: plan.requires_target, options: [{ value: "", label: plan.requires_target ? "Choose migration target" : "No migration target needed" }, ...plan.eligible_targets.map((target) => ({ value: target.id, label: `${target.name} · ${target.available_deployments} slots` }))] })}${plan.requires_target && !plan.eligible_targets.length ? html`<gw-alert tone="warning">No eligible migration target is available.</gw-alert>` : nothing}
              <div class="form-footer">
                <gw-button
                  type="submit"
                  variant="danger"
                  ?disabled=${this.busy || (plan.requires_target && !plan.eligible_targets.length)}
                  >Queue deletion</gw-button
                >
              </div>
            </form></gw-card
          >`;
      }
      const configuration = node.configuration.desired || node.configuration.effective,
        flat = configuration
          ? flattenConfiguration(configuration as unknown as Record<string, unknown>)
          : {},
        specs: FieldSpec[] = Object.entries(flat).map(([name, value]) => ({
          name,
          label: name.replaceAll("_", " "),
          type:
            typeof value === "boolean"
              ? "boolean"
              : typeof value === "number"
                ? "number"
                : Array.isArray(value)
                  ? "json"
                  : "text",
          readonly: name === "node.id",
          options:
            name === "security.gateway_authentication"
              ? [
                  { value: "token", label: "Token" },
                  { value: "none", label: "None" },
                ]
              : name === "runtime.backend"
                ? [
                    { value: "docker-socket", label: "Docker socket" },
                    { value: "podman-socket", label: "Podman socket" },
                  ]
                : name === "log.format"
                  ? [
                      { value: "pretty", label: "Pretty" },
                      { value: "json", label: "JSON" },
                    ]
                  : undefined,
        }));
      return html`${heading(node.name, "Node health, runtime configuration, and lifecycle.", html`<gw-button variant="ghost" href="/admin/nodes">Back to nodes</gw-button>`)}${this.error()}${this.tokenPanel()}
        <div class="row">
          ${badge(node.status)}${badge(node.healthy ? "online" : "offline")}${badge(node.configuration.status)}
        </div>
        <gw-card heading="Resources"
          >${details({ capacity: node.capacity, usage: node.usage, version: node.version, region: node.region, heartbeat: node.last_heartbeat_at, desired_revision: node.configuration.desired_revision, effective_revision: node.configuration.effective_revision })}</gw-card
        >${node.configuration.error ? html`<gw-alert tone="error">${node.configuration.error}</gw-alert>` : nothing}<gw-card
          heading="Configuration"
          description="Changes are applied after the Node writes the configuration and restarts."
          >${
            configuration
              ? html`<form
                  class="form"
                  @submit=${(event: Event) => {
                    const form = readForm(event);
                    void this.run(
                      () =>
                        adminApi.updateNodeConfiguration(
                          id,
                          expandConfiguration(
                            formValues(form, specs),
                          ) as unknown as NodeConfiguration,
                        ),
                      () => this.refresh(),
                      "Node configuration queued.",
                    );
                  }}
                >
                  <div class="grid">${specs.map((spec) => field(spec, flat[spec.name]))}</div>
                  <div class="form-footer">
                    <gw-button type="submit" .loading=${this.busy}>Apply configuration</gw-button>
                  </div>
                </form>`
              : html`<gw-alert tone="info"
                  >This node has not reported a configuration yet.</gw-alert
                >`
          }</gw-card
        >${node.deletion ? html`<gw-card heading="Deletion progress">${details(node.deletion as unknown as Record<string, unknown>)}</gw-card>` : nothing}<gw-card
          heading="Node credentials"
          ><gw-button
            variant="outline"
            ?disabled=${this.busy}
            @click=${async () => {
              if (
                await confirmAction(
                  "Rotate node token?",
                  "Update the node's runtime configuration with the new token.",
                  "Rotate token",
                )
              )
                void this.run(
                  () => adminApi.rotateNodeToken(id),
                  (result) => {
                    this.token = result.token;
                  },
                  "Node token rotated.",
                );
            }}
            >Rotate token</gw-button
          ></gw-card
        ><gw-card heading="Delete node"
          ><gw-button
            variant="danger"
            href=${`/admin/nodes/${id}/delete`}
            ?disabled=${Boolean(node.deletion && ["queued", "migrating", "draining", "deleting"].includes(node.deletion.status))}
            >Prepare deletion</gw-button
          ></gw-card
        >`;
    }
    return html`${heading("Nodes", "Build and serving capacity across your regions.", html`<gw-button href="/admin/nodes/new">Create node</gw-button>`)}${this.error()}<gw-card
        heading="Local node process"
        ><div class="stack">
          <div class="row">
            ${badge(data.local_process.process.state)}<span class="muted"
              >${data.local_process.process.pid ? `PID ${data.local_process.process.pid}` : ""}</span
            >
          </div>
          <p>
            ${data.local_process.process.message || "The Control API supervises the managed local node."}
          </p>
          <div class="actions">
            ${["start", "stop", "restart"].map(
              (action) =>
                html`<gw-button
                  variant="outline"
                  ?disabled=${this.busy || !data.local_process.managed}
                  @click=${() =>
                    this.run(
                      () => adminApi.localNodeProcess(action as "start" | "stop" | "restart"),
                      () => this.refresh(),
                      "Local process updated.",
                    )}
                  >${action}</gw-button
                >`,
            )}
          </div>
        </div></gw-card
      >${table(
        data.nodes,
        [
          {
            label: "Node",
            render: (node) => html`<a href=${`/admin/nodes/${node.id}`}>${node.name}</a>`,
          },
          { label: "Region", key: "region" },
          { label: "Health", render: (node) => badge(node.healthy ? "online" : node.status) },
          {
            label: "Capabilities",
            render: (node) =>
              [node.build_enabled ? "Build" : "", node.serve_enabled ? "Serve" : ""]
                .filter(Boolean)
                .join(" · "),
          },
          { label: "Configuration", render: (node) => badge(node.configuration.status) },
          {
            label: "Deployments",
            render: (node) => `${node.usage.deployments} / ${node.capacity.max_deployments}`,
          },
          {
            label: "Deletion",
            render: (node) => (node.deletion ? badge(node.deletion.status) : "—"),
          },
        ],
        "Nodes",
      )}`;
  }
}
define("gw-page-admin-nodes", NodesPage);
