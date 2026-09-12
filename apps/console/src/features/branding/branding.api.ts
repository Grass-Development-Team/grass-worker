import { request } from "@/lib/api";

interface SiteConfig {
  site_name: string;
  logo_url: string | null;
  version: string;
}

export const brandingApi = {
  getSiteConfig: () => request<SiteConfig>("/api/v1/site-config"),
};
