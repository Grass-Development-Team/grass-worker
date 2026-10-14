import { request } from "@/infra/api/index";
import type { AdminQuotaPlan, QuotaLimitInput } from "./model";
export const quotaPlansApi = {
  listQuotaPlans: () => request<{ plans: AdminQuotaPlan[] }>("/api/v1/admin/quota-plans"),

  createQuotaPlan: (input: {
    code: string;
    name: string;
    description?: string;
    limits: QuotaLimitInput[];
  }) =>
    request<{ plan: { id: string; code: string; name: string } }>("/api/v1/admin/quota-plans", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateQuotaPlan: (
    planId: string,
    input: {
      name?: string;
      description?: string;
      enabled?: boolean;
      is_default?: boolean;
      limits?: QuotaLimitInput[];
    },
  ) =>
    request<{ plan: { id: string } }>(`/api/v1/admin/quota-plans/${planId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),
};
