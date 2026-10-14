import { request } from "@/infra/api/index";
import type { AdminIdentityProvider } from "./model";
export const providersApi = {
  listIdentityProviders: () =>
    request<{ providers: AdminIdentityProvider[] }>("/api/v1/admin/identity-providers"),

  createIdentityProvider: (input: {
    slug: string;
    template: "google" | "apple" | "github" | "custom";
    name: string;
    client_id: string;
    client_secret: string;
    issuer_url?: string;
    authorization_url?: string;
    token_url?: string;
    userinfo_url?: string;
    jwks_url?: string;
    scopes?: string[];
    enabled?: boolean;
  }) =>
    request<{ provider: AdminIdentityProvider }>("/api/v1/admin/identity-providers", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateIdentityProvider: (
    providerId: string,
    input: Partial<
      Pick<
        AdminIdentityProvider,
        | "name"
        | "enabled"
        | "client_id"
        | "issuer_url"
        | "authorization_url"
        | "token_url"
        | "userinfo_url"
        | "jwks_url"
        | "scopes"
      >
    > & { client_secret?: string },
  ) =>
    request<{ provider: AdminIdentityProvider }>(`/api/v1/admin/identity-providers/${providerId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  deleteIdentityProvider: (providerId: string) =>
    request<{ deleted: true }>(`/api/v1/admin/identity-providers/${providerId}`, {
      method: "DELETE",
    }),
};
