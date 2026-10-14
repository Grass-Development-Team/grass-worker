import { QueryController } from "@/infra/query";
import { adminApi, type AdminBatchItemResult, type AdminProjectFilters } from "@/services/admin";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing, type PropertyValues } from "lit";
import { Page, badge, details, field, heading, readForm, table, text } from "../../base";
export class AdminProjectsPage extends Page {
  static properties = {
    ...Page.properties,
    filters: { state: true },
    selected: { state: true },
    results: { state: true },
    activityPage: { state: true },
  };
  filters: AdminProjectFilters = {};
  selected = new Set<string>();
  results: AdminBatchItemResult[] = [];
  activityPage = 1;
  private projects = new QueryController(this, {
    queryKey: ["admin", "projects", this.filters],
    queryFn: () => adminApi.listProjects(this.filters),
  });
  private project = new QueryController(this, {
    queryKey: ["admin", "project", "none"],
    queryFn: () => adminApi.getProject(this.context.params.projectId),
    enabled: false,
  });
  private deployments = new QueryController(this, {
    queryKey: ["admin", "project-deployments", "none"],
    queryFn: () => adminApi.listProjectDeployments(this.context.params.projectId),
    enabled: false,
  });
  private domains = new QueryController(this, {
    queryKey: ["admin", "project-domains", "none"],
    queryFn: () => adminApi.listProjectDomains(this.context.params.projectId),
    enabled: false,
  });
  private activity = new QueryController(this, {
    queryKey: ["admin", "project-activity", "none"],
    queryFn: () => adminApi.listProjectActivity(this.context.params.projectId, this.activityPage),
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues) {
    const id = this.context.params.projectId;
    if (changed.has("context") && id) {
      this.project.configure({
        queryKey: ["admin", "project", id],
        queryFn: () => adminApi.getProject(id),
      });
      this.deployments.configure({
        queryKey: ["admin", "project-deployments", id],
        queryFn: () => adminApi.listProjectDeployments(id),
        refetchInterval: 10000,
      });
      this.domains.configure({
        queryKey: ["admin", "project-domains", id],
        queryFn: () => adminApi.listProjectDomains(id),
      });
      this.loadActivity();
    }
  }
  private loadActivity() {
    this.activity.configure({
      queryKey: ["admin", "project-activity", this.context.params.projectId, this.activityPage],
      queryFn: () => adminApi.listProjectActivity(this.context.params.projectId, this.activityPage),
    });
  }
  private reason() {
    return (
      (
        this.querySelector("gw-input[name='reason']") as (HTMLElement & { value: string }) | null
      )?.value.trim() || undefined
    );
  }
  private action(operation: () => Promise<unknown>, message = "Operation completed.") {
    void this.run(
      operation,
      async () => {
        await Promise.all([
          this.project.refresh(),
          this.deployments.refresh(),
          this.domains.refresh(),
          this.activity.refresh(),
        ]);
      },
      message,
    );
  }
  render() {
    this.classList.add("page");
    const id = this.context.params.projectId;
    if (id) {
      const state = this.queryState(this.project);
      if (state) return state;
      const data = this.project.result.data!,
        project = data.project;
      return html`${heading(project.name, "Platform governance for deployments, domains, and project identity.", html`<gw-button variant="ghost" href="/admin/projects">Back to projects</gw-button>`)}${this.error()}
        <div class="row">
          ${badge(project.status)}<span class="muted">${data.team?.name || "No team"}</span>
        </div>
        <gw-card heading="Project identity"
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () => adminApi.updateProjectSlug(id, text(form, "slug"), this.reason()),
                () => this.project.refresh().then(() => undefined),
                "Project slug updated.",
              );
            }}
          >
            ${field({ name: "slug", label: "Slug", required: true }, project.slug)}${field({ name: "reason", label: "Reason", description: "Recorded with governance actions below." })}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Save slug</gw-button>
            </div>
          </form></gw-card
        ><gw-card heading="Lifecycle"
          ><div class="actions">
            ${
              project.status === "deleted"
                ? html`<gw-button
                    variant="outline"
                    @click=${() => this.action(() => adminApi.restoreProject(id), "Project restored.")}
                    >Restore project</gw-button
                  >`
                : html`<gw-button
                      variant="outline"
                      @click=${async () => {
                        if (
                          project.status === "archived" ||
                          (await confirmAction(
                            "Archive project?",
                            "New deployments will be blocked.",
                            "Archive",
                          ))
                        )
                          this.action(() =>
                            project.status === "archived"
                              ? adminApi.unarchiveProject(id)
                              : adminApi.archiveProject(id),
                          );
                      }}
                      >${project.status === "archived" ? "Unarchive" : "Archive"}</gw-button
                    ><gw-button
                      variant="danger"
                      @click=${async () => {
                        if (
                          await confirmAction(
                            "Delete project?",
                            "This affects its deployed sites and domains.",
                            "Delete",
                          )
                        )
                          this.action(() => adminApi.deleteProject(id), "Project deleted.");
                      }}
                      >Delete</gw-button
                    >`
            }
          </div></gw-card
        ><gw-card heading="Deployments"
          >${
            this.queryState(this.deployments) ??
            table(
              this.deployments.result.data!.deployments,
              [
                {
                  label: "Deployment",
                  render: (deployment) =>
                    html`<a href=${`/projects/${id}/deployments/${deployment.id}`}
                      >${deployment.commit_message || deployment.id.slice(0, 8)}</a
                    >`,
                },
                { label: "Environment", key: "environment" },
                { label: "Build", render: (deployment) => badge(deployment.build_status) },
                { label: "Serve", render: (deployment) => badge(deployment.serve_status) },
                { label: "Release", render: (deployment) => badge(deployment.release_status) },
                {
                  label: "Actions",
                  render: (deployment) =>
                    html`<div class="actions">
                      ${
                        deployment.release_status === "active" || deployment.release_pending
                          ? html`<gw-button
                              variant="ghost"
                              size="sm"
                              @click=${async () => {
                                if (
                                  await confirmAction(
                                    "Withdraw deployment?",
                                    "This deployment will stop serving traffic.",
                                    "Withdraw",
                                  )
                                )
                                  this.action(() =>
                                    adminApi.withdrawDeployment(deployment.id, this.reason()),
                                  );
                              }}
                              >Withdraw</gw-button
                            >`
                          : deployment.build_status === "ready"
                            ? html`<gw-button
                                variant="ghost"
                                size="sm"
                                ?disabled=${deployment.release_pending}
                                @click=${() => this.action(() => adminApi.republishDeployment(deployment.id, this.reason()), "Republish requested.")}
                                >Republish</gw-button
                              >`
                            : nothing
                      }
                    </div>`,
                },
              ],
              "Governed deployments",
            )
          }</gw-card
        ><gw-card heading="Domains"
          >${
            this.queryState(this.domains) ??
            table(
              this.domains.result.data!.domains,
              [
                { label: "Host", key: "host" },
                { label: "Status", render: (domain) => badge(domain.status) },
                { label: "Review", render: (domain) => badge(domain.review_status) },
                {
                  label: "Actions",
                  render: (domain) =>
                    html`<div class="actions">
                      ${domain.kind === "custom" && domain.review_status === "pending" ? html`<gw-button variant="ghost" size="sm" @click=${() => this.action(() => adminApi.approveDomain(domain.id, this.reason()), "Domain approved.")}>Approve</gw-button><gw-button variant="ghost" size="sm" @click=${() => this.action(() => adminApi.rejectDomain(domain.id, this.reason()), "Domain rejected.")}>Reject</gw-button>` : nothing}<gw-button
                        variant="ghost"
                        size="sm"
                        @click=${async () => {
                          if (
                            await confirmAction(
                              "Delete domain?",
                              `${domain.host} will no longer be attached.`,
                              "Delete",
                            )
                          )
                            this.action(
                              () => adminApi.deleteDomain(domain.id, this.reason()),
                              "Domain deleted.",
                            );
                        }}
                        >Delete</gw-button
                      >
                    </div>`,
                },
              ],
              "Governed domains",
            )
          }</gw-card
        ><gw-card heading="Activity"
          >${
            this.queryState(this.activity) ??
            html`${table(
                this.activity.result.data!.events,
                [
                  { label: "Time", key: "created_at" },
                  { label: "Action", key: "action" },
                  { label: "Result", render: (event) => badge(event.result) },
                  { label: "Reason", key: "reason" },
                ],
                "Project audit activity",
              )}<gw-pagination
                .page=${this.activityPage}
                .pages=${this.activity.result.data!.pagination.total_pages}
                @gw-page=${(event: CustomEvent<{ page: number }>) => {
                  this.activityPage = event.detail.page;
                  this.loadActivity();
                }}
              ></gw-pagination>`
          }</gw-card
        ><gw-card heading="Details"
          >${details({ id: project.id, uuid: project.uuid, runtime: project.runtime, repository: project.repository_url, created: project.created_at })}</gw-card
        >`;
    }
    const state = this.queryState(this.projects);
    if (state) return state;
    const projects = this.projects.result.data!.projects;
    return html`${heading("Projects", "Manage project lifecycle across all teams.")}${this.error()}
      <form
        class="grid"
        @submit=${(event: Event) => {
          const form = readForm(event);
          this.filters = {
            q: text(form, "q") || undefined,
            status: (text(form, "status") || undefined) as AdminProjectFilters["status"],
          };
          this.projects.configure({
            queryKey: ["admin", "projects", this.filters],
            queryFn: () => adminApi.listProjects(this.filters),
          });
          this.selected = new Set();
        }}
      >
        ${field({ name: "q", label: "Search" })}${field({ name: "status", label: "Status", options: [{ value: "", label: "All statuses" }, ...["active", "archived", "deleted"].map((value) => ({ value, label: value }))] })}
        <div class="form-footer">
          <gw-button type="submit" variant="outline">Apply filters</gw-button>
        </div>
      </form>
      <div class="actions">
        <gw-checkbox
          label="Select all"
          .checked=${projects.length > 0 && this.selected.size === projects.length}
          @gw-change=${(event: CustomEvent<{ checked: boolean }>) => (this.selected = event.detail.checked ? new Set(projects.map((project) => project.id)) : new Set())}
        ></gw-checkbox
        >${["archive", "unarchive", "delete"].map(
          (action) =>
            html`<gw-button
              variant="outline"
              size="sm"
              ?disabled=${!this.selected.size || this.busy}
              @click=${async () => {
                if (
                  action !== "unarchive" &&
                  !(await confirmAction(
                    `${action} selected projects?`,
                    "Per-project results will be shown.",
                    action,
                  ))
                )
                  return;
                void this.run(
                  () =>
                    adminApi.batchProjects({
                      action: action as "archive" | "unarchive" | "delete",
                      ids: [...this.selected],
                    }),
                  async (result) => {
                    this.results = result.results;
                    this.selected = new Set();
                    await this.projects.refresh();
                  },
                  "Batch operation processed.",
                );
              }}
              >${action}</gw-button
            >`,
        )}
      </div>
      ${table(
        projects,
        [
          {
            label: "Select",
            render: (project) =>
              html`<gw-checkbox
                label=${`Select ${project.name}`}
                .checked=${this.selected.has(project.id)}
                @gw-change=${(event: CustomEvent<{ checked: boolean }>) => {
                  const next = new Set(this.selected);
                  event.detail.checked ? next.add(project.id) : next.delete(project.id);
                  this.selected = next;
                }}
              ></gw-checkbox>`,
          },
          {
            label: "Project",
            render: (project) =>
              html`<a href=${`/admin/projects/${project.id}`}>${project.name}</a>`,
          },
          { label: "Team", render: (project) => project.team?.name || "—" },
          { label: "Runtime", key: "runtime" },
          { label: "Status", render: (project) => badge(project.status) },
        ],
        "All projects",
      )}${
        this.results.length
          ? table(
              this.results,
              [
                { label: "ID", key: "id" },
                { label: "Result", render: (item) => badge(item.success ? "success" : "failed") },
                { label: "Message", key: "message" },
              ],
              "Batch project results",
            )
          : nothing
      }`;
  }
}
define("gw-page-admin-projects", AdminProjectsPage);
