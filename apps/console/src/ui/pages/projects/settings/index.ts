import { projectsApi } from "@/services/projects";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing } from "lit";
import { details, field, heading, readForm, text } from "../../base";
import { ProjectPage } from "../shared";
export class ProjectSettingsPage extends ProjectPage {
  render() {
    this.classList.add("page");
    const state = this.queryState(this.project);
    if (state) return state;
    const project = this.project.result.data!.project;
    return html`${heading("Project settings", "General information and project lifecycle.")}${this.error()}<gw-card
        heading="Project name"
        ><form
          class="form"
          @submit=${(event: Event) => {
            const data = readForm(event);
            if (this.canEdit)
              void this.run(
                () => projectsApi.update(this.projectId, { name: text(data, "name") }),
                () => this.project.refresh().then(() => undefined),
              );
          }}
        >
          ${field({ name: "name", label: "Name", required: true, readonly: !this.canEdit }, project.name)}${this.canEdit ? html`<div class="form-footer"><gw-button type="submit" .loading=${this.busy}>Save name</gw-button></div>` : nothing}
        </form></gw-card
      ><gw-card
        heading="Build and deployment"
        description="Configure Git access, framework, and build commands."
        ><gw-button
          variant="outline"
          href=${`/projects/${this.projectId}/settings/build-and-deployment`}
          >Open build settings</gw-button
        ></gw-card
      ><gw-card heading="Project identity"
        >${details({ id: project.id, slug: project.slug, runtime: project.runtime })}</gw-card
      >${
        this.canManage
          ? html`<gw-card heading="Project lifecycle"
              ><div class="stack">
                <p>
                  Archiving stops new deployments. Deleting removes the project from active views.
                </p>
                <div class="row">
                  <gw-button
                    variant="outline"
                    .loading=${this.busy}
                    @click=${async () => {
                      if (
                        project.archived_at ||
                        (await confirmAction(
                          "Archive project?",
                          "New deployments will be blocked until you restore the project.",
                          "Archive",
                        ))
                      )
                        void this.run(
                          () =>
                            project.archived_at
                              ? projectsApi.unarchive(this.projectId)
                              : projectsApi.archive(this.projectId),
                          () => this.project.refresh().then(() => undefined),
                        );
                    }}
                    >${project.archived_at ? "Unarchive project" : "Archive project"}</gw-button
                  ><gw-button
                    variant="danger"
                    ?disabled=${this.busy}
                    @click=${async () => {
                      if (
                        await confirmAction(
                          "Delete project?",
                          `Delete ${project.name}? This operation affects its deployments and domains.`,
                          "Delete project",
                        )
                      )
                        void this.run(
                          () => projectsApi.softDelete(this.projectId),
                          () => this.go("/projects"),
                          "Project deleted.",
                        );
                    }}
                    >Delete project</gw-button
                  >
                </div>
              </div></gw-card
            >`
          : nothing
      }`;
  }
}
define("gw-page-project-settings", ProjectSettingsPage);
