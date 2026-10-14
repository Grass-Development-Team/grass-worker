import { QueryController } from "@/infra/query";
import { deploymentsApi, formatDuration, shortCommit } from "@/services/deployments";
import { define } from "@/ui/components/base";
import { html, nothing, type PropertyValues } from "lit";
import { badge, field, heading, readForm, table, text } from "../../base";
import { ProjectPage } from "../shared";
export class DeploymentsPage extends ProjectPage {
  static properties = {
    ...ProjectPage.properties,
    environment: { state: true },
    region: { state: true },
  };
  environment = "";
  region = "";
  private deployments = new QueryController(this, {
    queryKey: ["deployment", "none"],
    queryFn: () => deploymentsApi.list(this.projectId),
    enabled: false,
  });
  private nodes = new QueryController(this, {
    queryKey: ["serve-nodes", "none"],
    queryFn: () => deploymentsApi.serveNodes(this.projectId),
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues) {
    super.willUpdate(changed);
    if (changed.has("context") || changed.has("environment")) {
      this.deployments.configure({
        queryKey: ["deployment", this.projectId, "list", this.environment],
        queryFn: () =>
          deploymentsApi.list(
            this.projectId,
            this.environment
              ? { environment: this.environment as "production" | "preview" }
              : undefined,
          ),
        enabled: Boolean(this.projectId),
        refetchInterval: 10000,
      });
      this.nodes.configure({
        queryKey: ["project", this.projectId, "serve-nodes"],
        queryFn: () => deploymentsApi.serveNodes(this.projectId),
        enabled: Boolean(this.projectId) && this.context.path.endsWith("/new"),
      });
    }
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.project);
    if (state) return state;
    const project = this.project.result.data!.project,
      creating = this.context.path.endsWith("/new");
    if (creating) {
      if (!this.canEdit || project.archived_at)
        return html`<gw-alert tone="warning">You cannot deploy this project.</gw-alert>`;
      const nodes = this.nodes.result.data?.serve_nodes || [],
        regions = [...new Set(nodes.map((node) => node.region))],
        available = nodes.filter((node) => !this.region || node.region === this.region);
      return html`${heading("New deployment", "Choose an environment and deployment target.", html`<gw-button variant="ghost" href=${`/projects/${this.projectId}/deployments`}>Back to deployments</gw-button>`)}${this.error()}${
        this.queryState(this.nodes) ??
        html`<gw-card
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event),
                node = text(form, "serve_node_id");
              if (node && !available.some((item) => item.id === node && item.schedulable)) return;
              void this.run(
                () =>
                  deploymentsApi.create(this.projectId, {
                    environment: text(form, "environment") as "production" | "preview",
                    branch: text(form, "branch") || undefined,
                    region: text(form, "region") || undefined,
                    serve_node_id: node || undefined,
                  }),
                (result) =>
                  this.go(`/projects/${this.projectId}/deployments/${result.deployment.id}`),
                "Deployment started.",
              );
            }}
          >
            ${field(
              {
                name: "environment",
                label: "Environment",
                options: [
                  { value: "production", label: "Production" },
                  { value: "preview", label: "Preview" },
                ],
                required: true,
              },
              "production",
            )}${field({ name: "branch", label: "Git branch" }, project.default_branch || "main")}<gw-select
              name="region"
              label="Region"
              .value=${this.region}
              .options=${[{ value: "", label: "Automatic region" }, ...regions.map((value) => ({ value, label: value, disabled: !nodes.some((node) => node.region === value && node.schedulable) }))]}
              @gw-change=${(event: CustomEvent<{ value: string }>) => (this.region = event.detail.value)}
            ></gw-select
            >${field({ name: "serve_node_id", label: "Serve node", options: [{ value: "", label: "Automatic selection" }, ...available.map((node) => ({ value: node.id, label: `${node.name} · ${node.region}${node.overflow_only ? " · overflow" : ""}`, disabled: !node.schedulable }))] })}${!available.some((node) => node.schedulable) ? html`<gw-alert tone="warning">No schedulable serve node is available in this region.</gw-alert>` : nothing}
            <div class="form-footer">
              <gw-button
                type="submit"
                .loading=${this.busy}
                ?disabled=${!available.some((node) => node.schedulable)}
                >Deploy</gw-button
              >
            </div>
          </form></gw-card
        >`
      }`;
    }
    return html`${heading("Deployments", "Builds and releases for this project.", this.canEdit && !project.archived_at ? html`<gw-button href=${`/projects/${this.projectId}/deployments/new`}>New deployment</gw-button>` : nothing)}<gw-select
        label="Environment filter"
        style="max-width:240px"
        .value=${this.environment}
        .options=${[
          { value: "", label: "All environments" },
          { value: "production", label: "Production" },
          { value: "preview", label: "Preview" },
        ]}
        @gw-change=${(event: CustomEvent<{ value: string }>) => (this.environment = event.detail.value)}
      ></gw-select
      >${
        this.queryState(this.deployments) ??
        table(
          this.deployments.result.data!.deployments,
          [
            {
              label: "Deployment",
              render: (deployment) =>
                html`<a href=${`/projects/${this.projectId}/deployments/${deployment.id}`}
                    >${shortCommit(deployment.source.commit_hash)} ·
                    ${deployment.source.branch || "Deployment"}</a
                  >
                  <div class="muted">${deployment.source.commit_message || ""}</div>`,
            },
            { label: "Environment", key: "environment" },
            { label: "Build", render: (deployment) => badge(deployment.build_status) },
            { label: "Serve", render: (deployment) => badge(deployment.serve_status) },
            { label: "Release", render: (deployment) => badge(deployment.release_status) },
            {
              label: "Duration",
              render: (deployment) => formatDuration(deployment.duration_seconds),
            },
            {
              label: "Created",
              render: (deployment) => new Date(deployment.created_at).toLocaleString(),
            },
          ],
          "Deployments",
        )
      }`;
  }
}
define("gw-page-deployments", DeploymentsPage);
