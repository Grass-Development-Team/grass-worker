import { request } from "@/infra/api/index";
import type { AdministrationStatus } from "./model";
export const platformApi = {
  status: () => request<AdministrationStatus>("/api/v1/admin/status"),
};
