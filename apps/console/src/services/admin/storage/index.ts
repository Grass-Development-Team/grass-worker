import { request } from "@/infra/api/index";
import type { StorageInput } from "@/services/storage/form";
import type { AdminStorageMigration, AdminStorageState } from "./model";
export const storageApi = {
  getStorage: () => request<AdminStorageState>("/api/v1/admin/storage"),

  testStorage: (input: StorageInput) =>
    request<{ tested: true }>("/api/v1/admin/storage/test", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  createStorageMigration: (input: StorageInput) =>
    request<{ migration: AdminStorageMigration }>("/api/v1/admin/storage/migrations", {
      method: "POST",
      body: JSON.stringify(input),
    }),
};
