import { request } from "@/lib/api";

export interface SiteConfig {
  site_name: string;
  version: string;
}

export const brandingApi = {
  getSiteConfig: () => request<SiteConfig>("/api/v1/site-config"),
};
