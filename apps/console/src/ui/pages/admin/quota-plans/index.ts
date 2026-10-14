import { adminApi, type AdminQuotaPlan } from "@/services/admin";
import { DIMENSION_LABELS } from "@/services/quota";
import { define } from "@/ui/components/base";
import { html } from "lit";
import { badge, field } from "../../base";
import { ResourcePage, rows, type ResourceRow } from "../resource";
export class QuotaPlansPage extends ResourcePage {
  protected definition = {
    title: "Quota plans",
    description: "Configure resource limits. Blank limits mean unlimited.",
    base: "/admin/quota-plans",
    list: async () => rows((await adminApi.listQuotaPlans()).plans),
    fields: (row: ResourceRow | null) => [
      { name: "name", label: "Plan name", required: true },
      ...(!row ? [{ name: "code", label: "Code", required: true }] : []),
      { name: "description", label: "Description" },
      ...(row
        ? [
            { name: "enabled", label: "Enabled", type: "boolean" },
            { name: "is_default", label: "Default plan", type: "boolean" },
          ]
        : []),
    ],
    columns: [
      { label: "Name", key: "name" },
      { label: "Code", key: "code" },
      { label: "Status", render: (row: ResourceRow) => badge(Boolean(row.enabled)) },
      { label: "Default", render: (row: ResourceRow) => (row.is_default ? "Default" : "—") },
    ],
    save: (values: Record<string, unknown>, row: ResourceRow | null) =>
      row
        ? adminApi.updateQuotaPlan(row.id, values)
        : adminApi.createQuotaPlan(
            values as unknown as Parameters<typeof adminApi.createQuotaPlan>[0],
          ),
  };
  protected extraEditor(row: ResourceRow | null) {
    const plan = row as unknown as AdminQuotaPlan | null;
    return html`<gw-separator></gw-separator>
      <h3>Resource limits</h3>
      <div class="grid">
        ${Object.entries(DIMENSION_LABELS).map(([dimension, label]) => {
          const limit = plan?.limits.find((limit) => limit.dimension === dimension)?.limit_value;
          return field(
            {
              name: `limit:${dimension}`,
              label,
              type: "number",
              min: "0",
              description: dimension.includes("monthly")
                ? "Resets monthly. Leave blank for unlimited."
                : "Leave blank for unlimited.",
            },
            limit !== undefined && limit >= 0 ? limit : "",
          );
        })}
      </div>`;
  }
  protected values(data: FormData, row: ResourceRow | null) {
    return {
      ...super.values(data, row),
      limits: Object.keys(DIMENSION_LABELS).map((dimension) => ({
        dimension,
        limit_value:
          String(data.get(`limit:${dimension}`) || "") === ""
            ? null
            : Number(data.get(`limit:${dimension}`)),
      })),
    };
  }
}
define("gw-page-admin-quota-plans", QuotaPlansPage);
