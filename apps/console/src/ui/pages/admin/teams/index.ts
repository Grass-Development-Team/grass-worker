import { QueryController } from "@/infra/query";
import { adminApi, type AdminBatchItemResult, type AdminTeamFilters } from "@/services/admin";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing, type PropertyValues } from "lit";
import { Page, badge, details, field, heading, readForm, table, text } from "../../base";
export class AdminTeamsPage extends Page {
  static properties = {
    ...Page.properties,
    filters: { state: true },
    selected: { state: true },
    results: { state: true },
  };
  filters: AdminTeamFilters = {};
  selected = new Set<string>();
  results: AdminBatchItemResult[] = [];
  private teams = new QueryController(this, {
    queryKey: ["admin", "teams", this.filters],
    queryFn: () => adminApi.listTeams(this.filters),
  });
  private groups = new QueryController(this, {
    queryKey: ["admin", "team-group-options"],
    queryFn: adminApi.listTeamGroups,
  });
  private plans = new QueryController(this, {
    queryKey: ["admin", "quota-plan-options"],
    queryFn: adminApi.listQuotaPlans,
  });
  private users = new QueryController(this, {
    queryKey: ["admin", "owner-options"],
    queryFn: () => adminApi.listUsers(),
    enabled: window.location.pathname.endsWith("/new"),
  });
  private detail = new QueryController(this, {
    queryKey: ["admin", "team-detail", "none"],
    queryFn: () => adminApi.teamDetail(this.context.params.resourceId),
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues) {
    if (changed.has("context") && this.context.params.resourceId)
      this.detail.configure({
        queryKey: ["admin", "team-detail", this.context.params.resourceId],
        queryFn: () => adminApi.teamDetail(this.context.params.resourceId),
      });
  }
  private options() {
    return {
      groups: (this.groups.result.data?.groups || []).map((group) => ({
        value: group.id,
        label: group.name,
      })),
      plans: [
        { value: "", label: "Inherit from group" },
        ...(this.plans.result.data?.plans || []).map((plan) => ({
          value: plan.id,
          label: plan.name,
        })),
      ],
    };
  }
  private refresh() {
    return this.teams.refresh().then(() => undefined);
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.teams);
    if (state) return state;
    const teams = this.teams.result.data!.teams,
      creating = this.context.path.endsWith("/new"),
      id = this.context.params.resourceId,
      options = this.options();
    if (creating)
      return html`${heading("Create team", "Assign an existing account as the team owner.", html`<gw-button variant="ghost" href="/admin/teams">Back to teams</gw-button>`)}${this.error()}<gw-card
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () =>
                  adminApi.createTeam({
                    name: text(form, "name"),
                    slug: text(form, "slug") || undefined,
                    owner_user_id: text(form, "owner_user_id"),
                  }),
                async () => {
                  await this.refresh();
                  this.go("/admin/teams");
                },
                "Team created.",
              );
            }}
          >
            ${field({ name: "name", label: "Team name", required: true })}${field({ name: "slug", label: "Slug (optional)" })}${this.queryState(this.users) ?? field({ name: "owner_user_id", label: "Owner", required: true, options: [{ value: "", label: "Choose owner" }, ...this.users.result.data!.users.map((user) => ({ value: user.id, label: user.display_name ? `${user.display_name} (${user.email})` : user.email }))] })}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Create team</gw-button>
            </div>
          </form></gw-card
        >`;
    if (id) {
      const loading = this.queryState(this.detail);
      if (loading) return loading;
      const data = this.detail.result.data!,
        team = data.team;
      return html`${heading(team.name, "Team membership, group, and plan assignment.", html`<gw-button variant="ghost" href="/admin/teams">Back to teams</gw-button>`)}${this.error()}<gw-card
          heading="General"
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () => adminApi.updateTeam(id, { name: text(form, "name") }),
                () => this.detail.refresh().then(() => undefined),
              );
            }}
          >
            ${field({ name: "name", label: "Team name", required: true }, team.name)}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Save name</gw-button>
            </div>
          </form></gw-card
        ><gw-card heading="Group and quota"
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                async () => {
                  const group = text(form, "group_id");
                  if (group) await adminApi.assignTeamGroup(id, group);
                  return adminApi.setTeamQuotaPlan(id, text(form, "plan_id") || null);
                },
                () => this.detail.refresh().then(() => undefined),
              );
            }}
          >
            ${field({ name: "group_id", label: "Team group", options: options.groups }, team.group?.id || "")}${field({ name: "plan_id", label: "Plan override", options: options.plans }, team.explicit_quota_plan_id || "")}
            <p>Effective plan: ${data.quota_plan.name} · ${data.quota_plan.source}</p>
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Save assignment</gw-button>
            </div>
          </form></gw-card
        ><gw-card heading="Members"
          >${table(
            data.members,
            [
              { label: "Email", key: "email" },
              { label: "Name", key: "display_name" },
              { label: "Role", key: "role" },
              { label: "Joined", key: "joined_at" },
            ],
            "Team members",
          )}</gw-card
        ><gw-card heading="Team details"
          >${details({ kind: team.kind, slug: team.slug, projects: data.project_count, created: team.created_at })}</gw-card
        >${
          team.kind !== "personal"
            ? html`<gw-card heading="Delete team"
                ><gw-button
                  variant="danger"
                  ?disabled=${this.busy}
                  @click=${async () => {
                    if (
                      await confirmAction(
                        "Delete team?",
                        "This affects team membership and owned projects.",
                        "Delete team",
                      )
                    )
                      void this.run(
                        () => adminApi.deleteTeam(id),
                        () => this.go("/admin/teams"),
                        "Team deleted.",
                      );
                  }}
                  >Delete team</gw-button
                ></gw-card
              >`
            : nothing
        }`;
    }
    return html`${heading("Teams", "Manage shared and personal workspaces.", html`<gw-button href="/admin/teams/new">Create team</gw-button>`)}${this.error()}
      <form
        class="grid"
        @submit=${(event: Event) => {
          const form = readForm(event);
          this.filters = {
            q: text(form, "q") || undefined,
            kind: (text(form, "kind") || undefined) as AdminTeamFilters["kind"],
            group_id: text(form, "group_id") || undefined,
            quota_plan_id: text(form, "quota_plan_id") || undefined,
          };
          this.selected = new Set();
          this.teams.configure({
            queryKey: ["admin", "teams", this.filters],
            queryFn: () => adminApi.listTeams(this.filters),
          });
        }}
      >
        ${field({ name: "q", label: "Search" })}${field({
          name: "kind",
          label: "Kind",
          options: [
            { value: "", label: "All kinds" },
            { value: "team", label: "Shared team" },
            { value: "personal", label: "Personal" },
          ],
        })}${field({ name: "group_id", label: "Group", options: [{ value: "", label: "All groups" }, ...options.groups] })}${field({ name: "quota_plan_id", label: "Quota plan", options: [{ value: "", label: "All plans" }, ...options.plans.filter((option) => option.value)] })}
        <div class="form-footer">
          <gw-button type="submit" variant="outline">Apply filters</gw-button>
        </div>
      </form>
      <gw-card heading="Batch assignment"
        ><form
          class="form"
          @submit=${async (event: Event) => {
            const form = readForm(event),
              action = text(form, "action"),
              ids = [...this.selected];
            if (!ids.length) return;
            if (
              action === "delete" &&
              !(await confirmAction(
                "Delete selected teams?",
                "Personal teams cannot be deleted. Per-item results will be shown.",
                "Delete teams",
              ))
            )
              return;
            void this.run(
              () =>
                adminApi.batchTeams(
                  action === "assign_group"
                    ? { action, ids, group_id: text(form, "group_id") }
                    : action === "assign_quota_plan"
                      ? { action, ids, plan_id: text(form, "plan_id") || null }
                      : { action: "delete", ids },
                ),
              async (result) => {
                this.results = result.results;
                this.selected = new Set();
                await this.refresh();
              },
              "Batch operation processed.",
            );
          }}
        >
          <div class="grid">
            ${field(
              {
                name: "action",
                label: "Action",
                options: [
                  { value: "assign_group", label: "Assign group" },
                  { value: "assign_quota_plan", label: "Assign quota plan" },
                  { value: "delete", label: "Delete teams" },
                ],
              },
              "assign_group",
            )}${field({ name: "group_id", label: "Group", options: options.groups })}${field({ name: "plan_id", label: "Plan", options: options.plans })}
          </div>
          <div class="form-footer">
            <gw-checkbox
              label="Select all"
              .checked=${teams.length > 0 && this.selected.size === teams.length}
              @gw-change=${(event: CustomEvent<{ checked: boolean }>) => (this.selected = event.detail.checked ? new Set(teams.map((team) => team.id)) : new Set())}
            ></gw-checkbox
            ><gw-button
              type="submit"
              variant="outline"
              ?disabled=${!this.selected.size || this.busy}
              >Apply to ${this.selected.size} teams</gw-button
            >
          </div>
        </form></gw-card
      >${table(
        teams,
        [
          {
            label: "Select",
            render: (team) =>
              html`<gw-checkbox
                label=${`Select ${team.name}`}
                .checked=${this.selected.has(team.id)}
                @gw-change=${(event: CustomEvent<{ checked: boolean }>) => {
                  const next = new Set(this.selected);
                  event.detail.checked ? next.add(team.id) : next.delete(team.id);
                  this.selected = next;
                }}
              ></gw-checkbox>`,
          },
          {
            label: "Name",
            render: (team) => html`<a href=${`/admin/teams/${team.id}`}>${team.name}</a>`,
          },
          { label: "Kind", key: "kind" },
          { label: "Group", render: (team) => team.group?.name || "—" },
          { label: "Members", key: "member_count" },
        ],
        "Teams",
      )}${
        this.results.length
          ? table(
              this.results,
              [
                { label: "ID", key: "id" },
                { label: "Result", render: (item) => badge(item.success ? "success" : "failed") },
                { label: "Message", key: "message" },
              ],
              "Batch team results",
            )
          : nothing
      }`;
  }
}
define("gw-page-admin-teams", AdminTeamsPage);
