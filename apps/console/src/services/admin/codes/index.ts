import { request } from "@/infra/api/index";
import type { AdminCode, AdminCodesPage, AdminCodeStatus, AdminGeneratedCode } from "./model";
export const codesApi = {
  listCodes: (filters?: { scope?: string; status?: AdminCodeStatus; page?: number }) => {
    const query = new URLSearchParams();
    if (filters?.scope) query.set("scope", filters.scope);
    if (filters?.status) query.set("status", filters.status);
    if (filters?.page) query.set("page", String(filters.page));
    const suffix = query.size ? `?${query}` : "";
    return request<AdminCodesPage>(`/api/v1/admin/codes${suffix}`);
  },

  generateCodes: (input: {
    scope: string;
    count: number;
    expires_in_days: number | null;
    never_expires: boolean;
  }) =>
    request<{ codes: AdminGeneratedCode[] }>("/api/v1/admin/codes", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  revokeCode: (codeId: string) =>
    request<{ code: AdminCode }>(`/api/v1/admin/codes/${codeId}/revoke`, { method: "POST" }),
};
