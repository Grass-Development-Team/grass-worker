import { QueryController } from "@/infra/query";
import { deploymentsApi } from "@/services/deployments";
import { projectsApi } from "@/services/projects";
import { define } from "@/ui/components/base";
import type { PropertyValues } from "lit";
import { html, nothing } from "lit";
import { badge, details, heading } from "../../base";
import { ProjectPage } from "../shared";
export class ProjectOverviewPage extends ProjectPage {
  private deployments = new QueryController(this, {
    queryKey: ["deployment", "none"],
    queryFn: () => deploymentsApi.list(this.projectId),
    enabled: false,
  });
  private hosts = new QueryController(this, {
    queryKey: ["project-hosts", "none"],
    queryFn: () => projectsApi.listHosts(this.projectId),
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues) {
    super.willUpdate(changed);
    if (changed.has("context")) {
      this.deployments.configure({
        queryKey: ["deployment", this.projectId, "list"],
        queryFn: () => deploymentsApi.list(this.projectId),
        enabled: Boolean(this.projectId),
        refetchInterval: 10000,
      });
      this.hosts.configure({
        queryKey: ["project", this.projectId, "hosts"],
        queryFn: () => projectsApi.listHosts(this.projectId),
        enabled: Boolean(this.projectId),
        refetchInterval: 10000,
      });
    }
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.project);
    if (state) return state;
    const project = this.project.result.data!.project,
      latest = this.deployments.result.data?.deployments[0];
    return html`${heading(project.name, project.repository_url || "Connect a repository to deploy this project.", this.canEdit && !project.archived_at ? html`<gw-button href=${`/projects/${this.projectId}/deployments/new`}>Deploy</gw-button>` : nothing)}${project.archived_at ? html`<gw-alert tone="warning">This project is archived.</gw-alert>` : nothing}
      <div class="grid">
        <gw-card heading="Latest deployment"
          >${
            this.queryState(this.deployments) ??
            (latest
              ? html`<div class="stack">
                  <div class="row">
                    ${badge(latest.build_status)}${badge(latest.serve_status)}${badge(latest.release_status)}
                  </div>
                  <strong
                    >${latest.source.commit_message || latest.source.branch || "Deployment"}</strong
                  >
                  <p>${new Date(latest.created_at).toLocaleString()}</p>
                  ${latest.screenshot_url ? html`<gw-image src=${latest.screenshot_url} alt="Deployment screenshot"></gw-image>` : nothing}<gw-button
                    variant="outline"
                    href=${`/projects/${this.projectId}/deployments/${latest.id}`}
                    >View deployment</gw-button
                  >
                </div>`
              : html`<gw-empty-state heading="No deployments yet"></gw-empty-state>`)
          }</gw-card
        ><gw-card heading="Domains"
          >${this.queryState(this.hosts) ?? html`<div class="stack">${this.hosts.result.data!.hosts.map((host) => html`<div class="row"><a href=${`https://${host.host}`} target="_blank" rel="noopener noreferrer">${host.host}</a>${badge(host.connection_state || host.status)}</div>`)}${!this.hosts.result.data!.hosts.length ? html`<p>No domains assigned yet.</p>` : nothing}<gw-button variant="outline" href=${`/projects/${this.projectId}/domains`}>Manage domains</gw-button></div>`}</gw-card
        >
      </div>
      <gw-card heading="Project details"
        >${details({ slug: project.slug, runtime: project.runtime, branch: project.default_branch, root_directory: project.source_config.root_directory || ".", created: project.created_at })}</gw-card
      >`;
  }
}
define("gw-page-project-overview", ProjectOverviewPage);
