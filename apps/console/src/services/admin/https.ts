import { request } from "@/infra/api";
export interface HttpsSettings {
  issuer: "letsencrypt" | "zerossl";
  zerossl_eab_configured: boolean;
}
export const httpsApi = {
  get: () => request<HttpsSettings>("/api/v1/admin/domain-https"),
  update: (input: { issuer: HttpsSettings["issuer"]; eab_kid?: string; eab_hmac_key?: string }) =>
    request<HttpsSettings>("/api/v1/admin/domain-https", {
      method: "PATCH",
      body: JSON.stringify(input),
    }),
};
