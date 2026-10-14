import { QueryController } from "@/infra/query";
import { projectsApi } from "@/services/projects";
import { credentialMatchesRepository } from "@/services/projects/credentials";
import { teamsApi } from "@/services/teams";
import { define } from "@/ui/components/base";
import { html, nothing, type PropertyValues } from "lit";
import { field, heading, readForm, text } from "../../../base";
import { ProjectPage } from "../../shared";
export class BuildSettingsPage extends ProjectPage {
  private credentials = new QueryController(this, {
    queryKey: ["credentials", "none"],
    queryFn: () => teamsApi.listSourceCredentials(this.project.result.data!.project.team_id),
    enabled: false,
  });
  private binding = new QueryController(this, {
    queryKey: ["binding", "none"],
    queryFn: () => projectsApi.getSourceCredential(this.projectId),
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues) {
    super.willUpdate(changed);
    if (changed.has("context"))
      this.binding.configure({
        queryKey: ["project", this.projectId, "binding"],
        queryFn: () => projectsApi.getSourceCredential(this.projectId),
        enabled: Boolean(this.projectId),
      });
    const project = this.project.result.data;
    if (project)
      this.credentials.configure({
        queryKey: ["team", project.project.team_id, "credentials"],
        queryFn: () => teamsApi.listSourceCredentials(project.project.team_id),
        enabled: this.canManageCredentials,
      });
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.project);
    if (state) return state;
    const project = this.project.result.data!.project;
    const specs = [
      { name: "repository_url", label: "Repository URL" },
      { name: "default_branch", label: "Production branch" },
      { name: "root_directory", label: "Root directory" },
      { name: "install_command", label: "Install command" },
      { name: "build_command", label: "Build command" },
      { name: "output_directory", label: "Output directory" },
      { name: "framework_hint", label: "Framework hint" },
    ];
    return html`${heading("Build and deployment", "Configure how this project is fetched, built, and delivered.", html`<gw-button variant="ghost" href=${`/projects/${this.projectId}/settings`}>Back to settings</gw-button>`)}${this.error()}<gw-card
        heading="Git and build settings"
        ><form
          class="form"
          @submit=${(event: Event) => {
            const form = readForm(event);
            if (this.canEdit)
              void this.run(
                () =>
                  projectsApi.update(
                    this.projectId,
                    Object.fromEntries(specs.map((spec) => [spec.name, text(form, spec.name)])),
                  ),
                () => this.project.refresh().then(() => undefined),
              );
          }}
        >
          <div class="grid">
            ${specs.map((spec) => field({ ...spec, readonly: !this.canEdit }, spec.name === "root_directory" ? project.source_config.root_directory : spec.name === "framework_hint" ? project.source_config.framework_hint : project[spec.name as keyof typeof project]))}
          </div>
          ${this.canEdit ? html`<div class="form-footer"><gw-button type="submit" .loading=${this.busy}>Save build settings</gw-button></div>` : nothing}
        </form></gw-card
      ><gw-card
        heading="Private repository credential"
        description="Only active credentials matching the saved repository scheme, host, and port can be bound."
        >${
          this.queryState(this.binding) ??
          (this.canManageCredentials
            ? (this.queryState(this.credentials) ??
              html`<form
                class="form"
                @submit=${(event: Event) => {
                  const form = readForm(event),
                    id = text(form, "credential");
                  void this.run(
                    async () =>
                      id
                        ? projectsApi.bindSourceCredential(this.projectId, id)
                        : projectsApi.unbindSourceCredential(this.projectId),
                    () => this.binding.refresh().then(() => undefined),
                  );
                }}
              >
                ${field({ name: "credential", label: "Credential", options: [{ value: "", label: "Anonymous access" }, ...this.credentials.result.data!.credentials.filter((credential) => !credential.revoked_at && credentialMatchesRepository(credential, project.repository_url)).map((credential) => ({ value: credential.id, label: `${credential.name} · ${credential.host}:${credential.port}` }))] }, this.binding.result.data?.credential?.id || "")}
                <div class="form-footer">
                  <gw-button type="submit" variant="outline" .loading=${this.busy}
                    >Save binding</gw-button
                  >
                </div>
              </form>`)
            : html`<p>${this.binding.result.data?.credential?.name || "Anonymous access"}</p>`)
        }</gw-card
      >`;
  }
}
define("gw-page-project-build", BuildSettingsPage);
