import { QueryController } from "@/infra/query";
import { projectsApi } from "@/services/projects";
import { canCreateProject } from "@/services/teams/permissions";
import { teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { html, nothing } from "lit";
import { Page, badge, heading, table } from "../base";
export class ProjectsPage extends Page {
  private team = teams.value.active;
  private projects = new QueryController(this, {
    queryKey: ["team", this.team?.id, "projects"],
    queryFn: () => projectsApi.list(this.team!.id),
    enabled: Boolean(this.team),
  });
  render() {
    this.classList.add("page");
    if (!this.team)
      return html`<gw-empty-state heading="Create a team to start deploying"
        ><gw-button href="/teams/new">Create team</gw-button></gw-empty-state
      >`;
    return html`${heading("Projects", "Your team's deployments start here.", teams.value.role && canCreateProject(teams.value.role) ? html`<gw-button href="/projects/new"><gw-icon slot="icon" name="plus"></gw-icon>New project</gw-button>` : nothing)}${
      this.queryState(this.projects) ??
      table(
        this.projects.result.data!.projects,
        [
          {
            label: "Project",
            render: (project) =>
              html`<a href=${`/projects/${project.id}`}><strong>${project.name}</strong></a>
                <div class="muted">${project.slug}</div>`,
          },
          { label: "Runtime", key: "runtime" },
          { label: "Repository", render: (project) => project.repository_url || "Not connected" },
          {
            label: "Status",
            render: (project) =>
              badge(project.deleted_at ? "deleted" : project.archived_at ? "archived" : "active"),
          },
        ],
        "Projects",
      )
    }`;
  }
}
define("gw-page-projects", ProjectsPage);
