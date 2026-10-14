import { adminApi } from "@/services/admin";
import { define } from "@/ui/components/base";
import { badge } from "../../base";
import { ResourcePage, rows, type ResourceRow } from "../resource";
export class ProvidersPage extends ResourcePage {
  protected definition = {
    title: "Identity providers",
    description: "Configure external sign-in. Client secrets are write-only.",
    base: "/admin/settings/authentication/providers",
    list: async () => rows((await adminApi.listIdentityProviders()).providers),
    defaults: { enabled: true, template: "custom" },
    fields: (row: ResourceRow | null) => [
      ...(!row
        ? [
            {
              name: "template",
              label: "Provider template",
              options: ["google", "apple", "github", "custom"].map((value) => ({
                value,
                label: value,
              })),
            },
            { name: "slug", label: "Slug", required: true },
          ]
        : []),
      { name: "name", label: "Display name", required: true },
      { name: "client_id", label: "Client ID", required: true },
      {
        name: "client_secret",
        label: "Client secret",
        type: "password",
        required: !row,
        description: row ? "Leave blank to keep the configured secret." : "",
      },
      { name: "enabled", label: "Enabled", type: "boolean" },
      { name: "issuer_url", label: "Issuer URL", type: "url" },
      { name: "authorization_url", label: "Authorization URL", type: "url" },
      { name: "token_url", label: "Token URL", type: "url" },
      { name: "userinfo_url", label: "User info URL", type: "url" },
      { name: "jwks_url", label: "JWKS URL", type: "url" },
      { name: "scopes", label: "Scopes", description: "Separate scopes with spaces." },
    ],
    columns: [
      { label: "Name", key: "name" },
      { label: "Slug", key: "slug" },
      { label: "Kind", key: "kind" },
      { label: "Status", render: (row: ResourceRow) => badge(Boolean(row.enabled)) },
    ],
    save: (values: Record<string, unknown>, row: ResourceRow | null) => {
      if (!values.client_secret) delete values.client_secret;
      const scopes = String(values.scopes || "").trim();
      if (scopes) values.scopes = scopes.split(/\s+/);
      else delete values.scopes;
      for (const key of [
        "issuer_url",
        "authorization_url",
        "token_url",
        "userinfo_url",
        "jwks_url",
      ])
        if (!values[key]) delete values[key];
      return row
        ? adminApi.updateIdentityProvider(row.id, values)
        : adminApi.createIdentityProvider(
            values as unknown as Parameters<typeof adminApi.createIdentityProvider>[0],
          );
    },
    remove: (row: ResourceRow) => adminApi.deleteIdentityProvider(row.id),
  };
  protected editor(row: ResourceRow | null) {
    return super.editor(
      row ? { ...row, scopes: Array.isArray(row.scopes) ? row.scopes.join(" ") : row.scopes } : row,
    );
  }
}
define("gw-page-admin-providers", ProvidersPage);
