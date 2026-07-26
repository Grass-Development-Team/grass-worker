import { request } from "@/lib/api";

export interface AdministrationStatus {
  service: string;
  mode: "ready";
  version: string;
}

export const adminApi = {
  status: () => request<AdministrationStatus>("/api/v1/admin/status"),
};
