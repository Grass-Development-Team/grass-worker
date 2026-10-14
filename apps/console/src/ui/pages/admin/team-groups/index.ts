import { QueryController } from "@/infra/query";
import { adminApi, type AdminTeamGroup } from "@/services/admin";
import { define } from "@/ui/components/base";
import { html } from "lit";
import { badge, field } from "../../base";
import { ResourcePage, rows, type ResourceRow } from "../resource";
export class TeamGroupsPage extends ResourcePage {
  private plans = new QueryController(this, {
    queryKey: ["admin", "quota-plan-options"],
    queryFn: () => adminApi.listQuotaPlans(),
  });
  protected definition = {
    title: "Team groups",
    description: "Assign default plans and review policies to teams.",
    base: "/admin/team-groups",
    list: async () => rows((await adminApi.listTeamGroups()).groups),
    fields: (row: ResourceRow | null) => [
      { name: "name", label: "Group name", required: true },
      ...(!row ? [{ name: "code", label: "Code", required: true }] : []),
      { name: "description", label: "Description" },
      ...(row ? [{ name: "is_default", label: "Default group", type: "boolean" }] : []),
    ],
    columns: [
      { label: "Name", key: "name" },
      { label: "Code", key: "code" },
      { label: "Teams", key: "team_count" },
      { label: "Default", render: (row: ResourceRow) => badge(Boolean(row.is_default)) },
    ],
    save: (values: Record<string, unknown>, row: ResourceRow | null) =>
      row
        ? adminApi.updateTeamGroup(row.id, values)
        : adminApi.createTeamGroup(
            values as unknown as Parameters<typeof adminApi.createTeamGroup>[0],
          ),
    remove: (row: ResourceRow) => adminApi.deleteTeamGroup(row.id),
  };
  protected extraEditor(row: ResourceRow | null) {
    const group = row as unknown as AdminTeamGroup | null;
    const policies = [
      { value: "", label: "Inherit platform policy" },
      { value: "auto", label: "Automatic" },
      { value: "manual", label: "Manual approval" },
    ];
    return html`${this.queryState(this.plans) ?? field({ name: "quota_plan_id", label: "Quota plan", options: [{ value: "", label: "Platform default" }, ...this.plans.result.data!.plans.map((plan) => ({ value: plan.id, label: plan.name }))] }, group?.quota_plan_id)}
      <div class="grid">
        ${["production", "preview", "domain"].map((name) => field({ name: `review:${name}`, label: `${name} review`, options: policies }, group?.review_policy[name as keyof typeof group.review_policy] || ""))}
      </div>`;
  }
  protected values(data: FormData, row: ResourceRow | null) {
    return {
      ...super.values(data, row),
      quota_plan_id: String(data.get("quota_plan_id") || "") || null,
      review_policy: Object.fromEntries(
        ["production", "preview", "domain"].map((name) => [
          name,
          String(data.get(`review:${name}`) || "") || null,
        ]),
      ),
    };
  }
}
define("gw-page-admin-team-groups", TeamGroupsPage);
