import { queryClient } from "@/infra/query";
import { projectsApi } from "@/services/projects";
import { FRAMEWORK_PRESETS } from "@/services/projects/presets";
import { canCreateProject } from "@/services/teams/permissions";
import { teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { html } from "lit";
import { Page, field, heading, readForm, text } from "../../base";
export class CreateProjectPage extends Page {
  static properties = { ...Page.properties, preset: { state: true }, slug: { state: true } };
  preset = "vite";
  slug = "";
  private slugTouched = false;
  render() {
    this.classList.add("page");
    const team = teams.value.active,
      role = teams.value.role;
    if (!team || !role || !canCreateProject(role))
      return html`<gw-alert tone="warning"
          >Select a team where you have permission to create projects.</gw-alert
        ><gw-button href="/projects">Back to projects</gw-button>`;
    const preset =
      FRAMEWORK_PRESETS.find((item) => item.id === this.preset) || FRAMEWORK_PRESETS[0];
    return html`${heading("Create project", "Connect a Git repository and choose how to build it.", html`<gw-button variant="ghost" href="/projects">Cancel</gw-button>`)}${this.error()}
      <form
        class="form"
        @submit=${(event: Event) => {
          const form = readForm(event);
          void this.run(
            () =>
              projectsApi.create({
                team_id: team.id,
                name: text(form, "name"),
                slug: text(form, "slug"),
                runtime: text(form, "runtime") as "static" | "ssr",
                repository_url: text(form, "repository_url") || undefined,
                default_branch: text(form, "default_branch") || undefined,
                root_directory: text(form, "root_directory") || undefined,
                install_command: text(form, "install_command") || undefined,
                build_command: text(form, "build_command") || undefined,
                output_directory: text(form, "output_directory") || undefined,
                framework_hint: preset.id !== "custom" ? preset.id : undefined,
              }),
            async (result) => {
              await queryClient.invalidateQueries({ queryKey: ["team", team.id, "projects"] });
              this.go(`/projects/${result.project.id}`);
              if (!result.host_assignment.assigned && result.host_assignment.reason)
                this.failure = result.host_assignment.reason;
            },
            "Project created.",
          );
        }}
      >
        <gw-card heading="Project details"
          ><div class="grid">
            <gw-input
              name="name"
              label="Project name"
              required
              @gw-input=${(event: CustomEvent<{ value: string }>) => {
                if (!this.slugTouched)
                  this.slug = event.detail.value
                    .toLowerCase()
                    .replace(/[^a-z0-9]+/g, "-")
                    .replace(/^-+|-+$/g, "");
              }}
            ></gw-input
            ><gw-input
              name="slug"
              label="Project slug"
              .value=${this.slug}
              required
              pattern="[a-z0-9]+(-[a-z0-9]+)*"
              @gw-input=${(event: CustomEvent<{ value: string }>) => {
                this.slugTouched = true;
                this.slug = event.detail.value;
              }}
            ></gw-input
            >${field({ name: "repository_url", label: "Git repository URL", placeholder: "HTTPS, SSH, or git:// repository URL" })}${field({ name: "default_branch", label: "Default branch" }, "main")}
          </div></gw-card
        ><gw-card heading="Framework preset"
          ><div class="preset-grid">
            ${FRAMEWORK_PRESETS.map((item) => html`<button type="button" class="preset" aria-pressed=${String(item.id === this.preset)} @click=${() => (this.preset = item.id)}><strong>${item.label}</strong><span>${item.description}</span></button>`)}
          </div></gw-card
        ><gw-card
          heading="Build settings"
          description="Customize the preset or leave commands blank for auto-detection."
          ><div class="grid">
            ${field(
              {
                name: "runtime",
                label: "Runtime",
                required: true,
                options: [
                  { value: "static", label: "Static site" },
                  { value: "ssr", label: "Server-rendered application" },
                ],
              },
              preset.runtime,
            )}${field({ name: "root_directory", label: "Root directory" }, "")}${field({ name: "install_command", label: "Install command" }, preset.install)}${field({ name: "build_command", label: "Build command" }, preset.build)}${field({ name: "output_directory", label: "Output directory" }, preset.output)}
          </div></gw-card
        >
        <div class="form-footer">
          <gw-button type="submit" size="lg" .loading=${this.busy}>Create project</gw-button>
        </div>
      </form>`;
  }
}
define("gw-page-project-new", CreateProjectPage);
