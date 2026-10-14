import { QueryController } from "@/infra/query";
import { adminApi, type AdminHostSource, type HostSourceKind } from "@/services/admin";
import { regionsApi } from "@/services/regions";
import { define } from "@/ui/components/base";
import { html, nothing } from "lit";
import { badge, field, text } from "../../base";
import { ResourcePage, rows, type ResourceRow } from "../resource";
export class HostSourcesPage extends ResourcePage {
  static properties = {
    ...ResourcePage.properties,
    kind: { state: true },
    provider: { state: true },
  };
  kind = "wildcard";
  provider = "cloudflare";
  private regions = new QueryController(this, { queryKey: ["regions"], queryFn: regionsApi.list });
  protected definition = {
    title: "Host sources",
    description: "Platform domain pools, wildcard routing, and DNS providers.",
    base: "/admin/host-sources",
    list: async () => rows((await adminApi.listHostSources()).sources),
    defaults: { enabled: true, allows_auto_assign: true },
    fields: (row: ResourceRow | null) => [
      { name: "label", label: "Label", required: true },
      ...(!row ? [{ name: "base_domain", label: "Base domain", required: true }] : []),
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
      { name: "is_default", label: "Default source", type: "boolean" },
      ...(row
        ? [
            { name: "enabled", label: "Enabled", type: "boolean" },
            { name: "allows_auto_assign", label: "Allow automatic assignment", type: "boolean" },
          ]
        : []),
    ],
    columns: [
      { label: "Label", key: "label" },
      { label: "Domain", key: "base_domain" },
      { label: "Region", key: "region" },
      { label: "Kind", key: "kind" },
      { label: "Status", render: (row: ResourceRow) => badge(Boolean(row.enabled)) },
    ],
    save: (values: Record<string, unknown>, row: ResourceRow | null) =>
      row
        ? adminApi.updateHostSource(row.id, values)
        : adminApi.createHostSource(
            values as unknown as Parameters<typeof adminApi.createHostSource>[0],
          ),
    remove: (row: ResourceRow) => adminApi.removeHostSource(row.id),
  };
  protected extraEditor(row: ResourceRow | null) {
    const source = row as unknown as AdminHostSource | null,
      kind = source?.kind || this.kind,
      provider = source?.provider || this.provider,
      keys = source?.config_keys || [],
      required = !source;
    const specs =
      provider === "cloudflare"
        ? [
            {
              name: "api_token",
              label: "API token",
              type: "password",
              required: required && !keys.includes("api_token"),
            },
            { name: "zone_id", label: "Zone ID", required },
          ]
        : provider === "dnspod"
          ? [
              { name: "secret_id", label: "Secret ID", type: "password", required },
              { name: "secret_key", label: "Secret key", type: "password", required },
              { name: "record_line", label: "Record line" },
              { name: "ttl", label: "TTL", type: "number", min: "1" },
            ]
          : [
              { name: "access_key_id", label: "Access key ID", type: "password", required },
              { name: "secret_access_key", label: "Secret access key", type: "password", required },
              { name: "hosted_zone_id", label: "Hosted zone ID", required },
              { name: "region", label: "AWS region" },
              { name: "ttl", label: "TTL", type: "number", min: "1" },
            ];
    return html`${!row ? html`<gw-select name="kind" label="Source kind" .value=${this.kind} .options=${["wildcard", "dns_provider", "manual"].map((value) => ({ value, label: value.replaceAll("_", " ") }))} @gw-change=${(event: CustomEvent<{ value: string }>) => (this.kind = event.detail.value)}></gw-select>` : html`<p class="muted">${source!.kind} · ${source!.base_domain}</p>`}${
      kind === "dns_provider"
        ? html`${!row ? html`<gw-select name="provider" label="DNS provider" .value=${provider} .options=${["cloudflare", "dnspod", "route53"].map((value) => ({ value, label: value }))} @gw-change=${(event: CustomEvent<{ value: string }>) => (this.provider = event.detail.value)}></gw-select>` : html`<p class="muted">${provider} · configured keys: ${keys.join(", ") || "None"}. Blank fields preserve saved values.</p>`}
            <div class="grid">
              ${specs.map((spec) => field({ ...spec, name: `config:${spec.name}` }))}${field({ name: "config:record_value", label: "Record value", required })}
            </div>
            ${field({ name: "config:record_type", label: "Record type", options: ["A", "AAAA", "CNAME"].map((value) => ({ value, label: value })) }, "A")}${provider === "cloudflare" ? field({ name: "config:proxied", label: "Proxy through Cloudflare", type: "boolean" }, false) : nothing}${row ? field({ name: "update_shape", label: "Also update record type and proxy settings", type: "boolean", description: "Keep this off to preserve saved record shape." }, false) : nothing}`
        : nothing
    }`;
  }
  protected values(data: FormData, row: ResourceRow | null) {
    const values = super.values(data, row),
      source = row as unknown as AdminHostSource | null,
      kind = (source?.kind || this.kind) as HostSourceKind,
      provider = source?.provider || this.provider;
    values.kind = kind;
    if (kind === "dns_provider") {
      const config: Record<string, unknown> = {};
      for (const [name, value] of data.entries()) {
        if (!name.startsWith("config:")) continue;
        const key = name.slice(7);
        if (["record_type", "proxied"].includes(key)) continue;
        const valueText = String(value).trim();
        if (valueText) config[key] = key === "ttl" ? Number(valueText) : valueText;
      }
      if (!row || data.has("update_shape")) {
        config.record_type = text(data, "config:record_type");
        if (provider === "cloudflare") config.proxied = data.has("config:proxied");
      }
      if (Object.keys(config).length) {
        values.provider = provider;
        values.config = config;
      }
    }
    return values;
  }
}
define("gw-page-admin-host-sources", HostSourcesPage);
