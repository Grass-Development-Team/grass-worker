import { request } from "@/infra/api/index";
import type { AdminHostSource, HostSourceKind } from "./model";
export const hostSourcesApi = {
  listHostSources: () => request<{ sources: AdminHostSource[] }>("/api/v1/admin/host-sources"),

  createHostSource: (input: {
    label: string;
    kind: HostSourceKind;
    base_domain: string;
    region?: string;
    is_default?: boolean;
    provider?: string;
    config?: Record<string, unknown>;
  }) =>
    request<{ source: AdminHostSource }>("/api/v1/admin/host-sources", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateHostSource: (
    sourceId: string,
    input: Partial<
      Pick<AdminHostSource, "label" | "region" | "enabled" | "allows_auto_assign" | "is_default">
    > & {
      provider?: string;
      /** Shallow-merged server side; a null value deletes the key. */
      config?: Record<string, unknown>;
    },
  ) =>
    request<{ source: AdminHostSource }>(`/api/v1/admin/host-sources/${sourceId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  removeHostSource: (sourceId: string) =>
    request<{ ok: true }>(`/api/v1/admin/host-sources/${sourceId}`, { method: "DELETE" }),
};
