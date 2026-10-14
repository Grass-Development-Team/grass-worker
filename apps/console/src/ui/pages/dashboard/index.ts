import { QueryController } from "@/infra/query";
import { announcementsApi } from "@/services/announcements";
import { projectsApi } from "@/services/projects";
import { dimensionLabel, quotaApi } from "@/services/quota";
import { canCreateProject } from "@/services/teams/permissions";
import { teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { html, nothing } from "lit";
import { Page, badge, heading, table } from "../base";
export class DashboardPage extends Page {
  private team = teams.value.active;
  private projects = new QueryController(this, {
    queryKey: ["team", this.team?.id, "projects"],
    queryFn: () => projectsApi.list(this.team!.id),
    enabled: Boolean(this.team),
  });
  private quota = new QueryController(this, {
    queryKey: ["team", this.team?.id, "quota"],
    queryFn: () => quotaApi.usage(this.team!.id),
    enabled: Boolean(this.team),
  });
  private announcements = new QueryController(this, {
    queryKey: ["announcements"],
    queryFn: () => announcementsApi.list(),
  });
  render() {
    this.classList.add("page");
    if (!this.team)
      return html`${heading("Your workspace")}<gw-empty-state heading="Create your first team"
          ><gw-button href="/teams/new">Create team</gw-button></gw-empty-state
        >`;
    const projects = this.projects.result.data?.projects || [];
    return html`${heading("Overview", `Your projects and activity in ${this.team.name}.`, teams.value.role && canCreateProject(teams.value.role) ? html`<gw-button href="/projects/new"><gw-icon slot="icon" name="plus"></gw-icon>New project</gw-button>` : nothing)}
      <div class="stats">
        <div class="stat">
          <p>Projects</p>
          <strong>${this.projects.result.isPending ? "—" : projects.length}</strong>
        </div>
        <div class="stat">
          <p>Team</p>
          <strong style="font-size:20px">${this.team.name}</strong>
        </div>
        <div class="stat">
          <p>Plan</p>
          <strong style="font-size:20px">${this.quota.result.data?.plan.name || "—"}</strong>
        </div>
      </div>
      <gw-card heading="Recent projects"
        >${
          this.queryState(this.projects) ??
          table(
            projects.slice(0, 6),
            [
              {
                label: "Project",
                render: (project) => html`<a href=${`/projects/${project.id}`}>${project.name}</a>`,
              },
              { label: "Runtime", key: "runtime" },
              {
                label: "Status",
                render: (project) =>
                  badge(
                    project.deleted_at ? "deleted" : project.archived_at ? "archived" : "active",
                  ),
              },
            ],
            "Recent projects",
          )
        }</gw-card
      >${
        this.quota.result.data
          ? html`<gw-card heading="Usage"
              ><div class="grid">
                ${this.quota.result.data.usage.slice(0, 4).map(
                  (entry) =>
                    html`<div>
                      <p class="muted">
                        ${dimensionLabel(entry.dimension)} · ${entry.used} /
                        ${entry.limit ?? "Unlimited"}
                      </p>
                      <gw-progress
                        .value=${entry.limit ? (entry.used / entry.limit) * 100 : 0}
                        label=${dimensionLabel(entry.dimension)}
                      ></gw-progress>
                    </div>`,
                )}
              </div></gw-card
            >`
          : nothing
      }<gw-card heading="Announcements"
        >${
          this.queryState(this.announcements) ??
          html`${
            this.announcements.result.data?.announcements.map(
              (item) =>
                html`<article class="list-item">
                  <div>
                    <h3>${item.title}</h3>
                    <p style="white-space:pre-wrap">${item.content}</p>
                  </div>
                </article>`,
            ) || nothing
          }${!this.announcements.result.data?.announcements.length ? html`<p class="muted">No announcements yet.</p>` : nothing}`
        }</gw-card
      >`;
  }
}
define("gw-page-dashboard", DashboardPage);
