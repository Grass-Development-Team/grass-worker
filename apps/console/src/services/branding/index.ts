import { request } from "@/infra/api/index";
import type { SiteConfig } from "./model";
export type * from "./model";

export const brandingApi = {
  getSiteConfig: () => request<SiteConfig>("/api/v1/site-config"),
};
