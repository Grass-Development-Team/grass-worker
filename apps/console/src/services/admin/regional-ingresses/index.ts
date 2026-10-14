import { request } from "@/infra/api/index";
import type { AdminRegionalIngress } from "./model";
export const regionalIngressesApi = {
  listRegionalIngresses: () =>
    request<{ regional_ingresses: AdminRegionalIngress[] }>("/api/v1/admin/regional-ingresses"),

  createRegionalIngress: (input: {
    region: string;
    hostname: string;
    enabled?: boolean;
    health_check_path?: string;
    health_check_interval_seconds?: number;
  }) =>
    request<{ regional_ingress: AdminRegionalIngress }>("/api/v1/admin/regional-ingresses", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateRegionalIngress: (
    id: string,
    input: Partial<
      Pick<
        AdminRegionalIngress,
        "hostname" | "enabled" | "health_check_path" | "health_check_interval_seconds"
      >
    >,
  ) =>
    request<{ regional_ingress: AdminRegionalIngress }>(`/api/v1/admin/regional-ingresses/${id}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  removeRegionalIngress: (id: string) =>
    request<{ ok: true }>(`/api/v1/admin/regional-ingresses/${id}`, { method: "DELETE" }),
};
