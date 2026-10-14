import { request } from "@/infra/api/index";
import type {
  AdminBatchItemResult,
  AdminUser,
  AdminUserFilters,
  AdminUserMfaPolicy,
  AdminUserMfaResponse,
} from "./model";
export const usersApi = {
  listUsers: (filters: AdminUserFilters = {}) => {
    const query = new URLSearchParams();
    if (filters.q?.trim()) query.set("q", filters.q.trim());
    if (filters.status) query.set("status", filters.status);
    if (filters.role) query.set("role", filters.role);
    const suffix = query.size ? `?${query}` : "";
    return request<{ users: AdminUser[] }>(`/api/v1/admin/users${suffix}`);
  },

  batchUsers: (input: { action: "enable" | "disable"; ids: string[] }) =>
    request<{ results: AdminBatchItemResult[] }>("/api/v1/admin/users/batch", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  createUser: (input: {
    email: string;
    display_name?: string;
    platform_role?: "user" | "admin";
    password?: string;
  }) =>
    request<{ user: AdminUser; password: string | null }>("/api/v1/admin/users", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateUser: (
    userId: string,
    input: {
      display_name?: string | null;
      status?: "active" | "disabled";
      platform_role?: "user" | "admin";
    },
  ) =>
    request<{ user: AdminUser }>(`/api/v1/admin/users/${userId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  resetUserPassword: (userId: string, password?: string) =>
    request<{ user_id: string; password: string | null }>(
      `/api/v1/admin/users/${userId}/reset-password`,
      {
        method: "POST",
        body: JSON.stringify(password ? { password } : {}),
      },
    ),

  listUserMfa: (userId: string) =>
    request<AdminUserMfaResponse>(`/api/v1/admin/users/${userId}/mfa`),

  updateUserMfaPolicy: (userId: string, policy: AdminUserMfaPolicy) =>
    request<Pick<AdminUserMfaResponse, "policy" | "effective_requirements">>(
      `/api/v1/admin/users/${userId}/mfa`,
      {
        method: "PATCH",
        body: JSON.stringify(policy),
      },
    ),

  resetUserMfaFactor: (userId: string, factorId: string) =>
    request<{ deleted: true }>(`/api/v1/admin/users/${userId}/mfa/${factorId}`, {
      method: "DELETE",
    }),
};
