import { request } from "@/infra/api/index";
import type { Region } from "./model";
export type * from "./model";

export const regionsApi = {
  list: () => request<{ regions: Region[] }>("/api/v1/regions"),
  create: (input: { code: string; name?: string }) =>
    request<{ region: Region }>("/api/v1/admin/regions", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  rename: (code: string, name: string) =>
    request<{ ok: true }>(`/api/v1/admin/regions/${encodeURIComponent(code)}`, {
      method: "PATCH",
      body: JSON.stringify({ name }),
    }),
  remove: (code: string) =>
    request<{ ok: true }>(`/api/v1/admin/regions/${encodeURIComponent(code)}`, {
      method: "DELETE",
    }),
};
