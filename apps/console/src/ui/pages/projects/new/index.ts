import { queryClient } from "@/infra/query";
import { projectsApi } from "@/services/projects";
import { FRAMEWORK_PRESETS } from "@/services/projects/presets";
import { canCreateProject } from "@/services/teams/permissions";
import { teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { html, nothing } from "lit";
import { Page, field, readForm, text } from "../../base";
import "./style.css";

export class CreateProjectPage extends Page {
  static properties = { ...Page.properties, preset: { state: true }, slug: { state: true } };
  preset = "vite";
  slug = "";
  private slugTouched = false;

  render() {
    this.classList.add("project-create-page");
    const team = teams.value.active,
      role = teams.value.role;
    if (!team || !role || !canCreateProject(role))
      return html`<div class="project-create-access">
        <gw-alert tone="warning"
          >Select a team where you have permission to create projects.</gw-alert
        ><gw-button href="/projects">Back to projects</gw-button>
      </div>`;
    const preset =
      FRAMEWORK_PRESETS.find((item) => item.id === this.preset) || FRAMEWORK_PRESETS[0];
    return html`<form
      class="project-create-form"
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
      <section class="project-create-details" aria-labelledby="project-details-title">
        <header class="project-create-heading">
          <p class="project-create-eyebrow">Start with the basics</p>
          <h1 id="project-details-title">Configure Project</h1>
          <p>
            Connect a repository and choose a framework preset. You can refine the build settings
            before creating your project.
          </p>
        </header>
        ${this.error()}
        <div class="project-create-fields">
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
          ></gw-input>
          <gw-input
            name="slug"
            label="Project slug"
            .value=${this.slug}
            required
            pattern="[a-z0-9]+(-[a-z0-9]+)*"
            @gw-input=${(event: CustomEvent<{ value: string }>) => {
              this.slugTouched = true;
              this.slug = event.detail.value;
            }}
          ></gw-input>
          ${field({ name: "repository_url", label: "Git repository URL", placeholder: "HTTPS, SSH, or git:// repository URL" })}
          ${field({ name: "default_branch", label: "Default branch" }, "main")}
          <details class="project-create-build">
            <summary>Build settings<gw-icon name="down"></gw-icon></summary>
            <p>Customize the preset or leave commands blank for auto-detection.</p>
            <div class="grid">
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
              )}
              ${field({ name: "root_directory", label: "Root directory" }, "")}
              ${field({ name: "install_command", label: "Install command" }, preset.install)}
              ${field({ name: "build_command", label: "Build command" }, preset.build)}
              ${field({ name: "output_directory", label: "Output directory" }, preset.output)}
            </div>
          </details>
        </div>
      </section>
      <section class="project-create-presets" aria-labelledby="project-presets-title">
        <header class="project-create-heading">
          <p class="project-create-eyebrow">Choose your stack</p>
          <h2 id="project-presets-title">Framework Presets</h2>
          <p>Start from a known build setup, or choose custom and configure it later.</p>
        </header>
        <div class="project-create-preset-grid">
          ${FRAMEWORK_PRESETS.map(
            (item) =>
              html`<button
                type="button"
                class="project-create-preset"
                aria-pressed=${String(item.id === this.preset)}
                @click=${() => (this.preset = item.id)}
              >
                <span class="project-create-preset-icons"
                  ><span class="project-create-preset-icon"><gw-icon name="code"></gw-icon></span
                  >${item.id === this.preset ? html`<gw-icon name="check"></gw-icon>` : nothing}</span
                >
                <span class="project-create-preset-copy"
                  ><strong>${item.label}</strong><span>${item.description}</span></span
                >
              </button>`,
          )}
        </div>
      </section>
      <div class="project-create-submit">
        <gw-button type="submit" size="lg" .loading=${this.busy}>Create project</gw-button>
      </div>
    </form>`;
  }
}
define("gw-page-project-new", CreateProjectPage);
