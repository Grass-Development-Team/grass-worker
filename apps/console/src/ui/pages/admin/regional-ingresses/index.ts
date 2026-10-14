import { QueryController } from "@/infra/query";
import { adminApi } from "@/services/admin";
import { regionsApi } from "@/services/regions";
import { define } from "@/ui/components/base";
import { html } from "lit";
import { badge, details } from "../../base";
import { ResourcePage, rows, type ResourceRow } from "../resource";
export class IngressesPage extends ResourcePage {
  private regions = new QueryController(this, { queryKey: ["regions"], queryFn: regionsApi.list });
  protected definition = {
    title: "Regional ingresses",
    description: "Configure regional public entry hostnames and health checks.",
    base: "/admin/regional-ingresses",
    list: async () => rows((await adminApi.listRegionalIngresses()).regional_ingresses),
    defaults: { enabled: true, health_check_path: "/health", health_check_interval_seconds: 30 },
    fields: (row: ResourceRow | null) => [
      ...(!row
        ? [
            {
              name: "region",
              label: "Region",
              required: true,
              options: [
                { value: "", label: "Choose region" },
                ...(this.regions.result.data?.regions || []).map((region) => ({
                  value: region.code,
                  label: region.name || region.code,
                })),
              ],
            },
          ]
        : []),
      { name: "hostname", label: "Hostname", required: true },
      { name: "enabled", label: "Enabled", type: "boolean" },
      { name: "health_check_path", label: "Health check path", required: true },
      {
        name: "health_check_interval_seconds",
        label: "Health check interval (seconds)",
        type: "number",
        required: true,
        min: "1",
      },
    ],
    columns: [
      { label: "Hostname", key: "hostname" },
      { label: "Region", key: "region" },
      { label: "Enabled", render: (row: ResourceRow) => badge(Boolean(row.enabled)) },
      { label: "DNS", render: (row: ResourceRow) => badge(String(row.dns_status)) },
      {
        label: "Healthy nodes",
        render: (row: ResourceRow) =>
          Array.isArray(row.healthy_nodes) ? row.healthy_nodes.length : 0,
      },
    ],
    save: (values: Record<string, unknown>, row: ResourceRow | null) =>
      row
        ? adminApi.updateRegionalIngress(row.id, values)
        : adminApi.createRegionalIngress(
            values as unknown as Parameters<typeof adminApi.createRegionalIngress>[0],
          ),
    remove: (row: ResourceRow) => adminApi.removeRegionalIngress(row.id),
  };
  protected extraEditor(row: ResourceRow | null) {
    return row
      ? html`<gw-card heading="Connection status"
          >${details({ dns_status: row.dns_status, dns_error: row.dns_error, node_statuses: row.node_statuses, healthy_nodes: row.healthy_nodes })}</gw-card
        >`
      : html``;
  }
}
define("gw-page-admin-ingresses", IngressesPage);
