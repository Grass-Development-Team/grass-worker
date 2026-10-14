import { Store } from "@/infra/state";
import { brandingApi } from ".";
import type { SiteConfig } from "./model";
export const branding = new Store<SiteConfig>({
  site_name: "Grass Worker",
  logo_url: null,
  version: "0.1.0",
});
export async function refreshBranding(): Promise<void> {
  try {
    branding.set(await brandingApi.getSiteConfig());
  } catch {
    /* Keep the local brand while the API initializes. */
  }
}
