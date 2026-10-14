import { request } from "@/infra/api/index";
import type { AdminReview } from "./model";
export const reviewsApi = {
  listReviews: () => request<{ total: number; reviews: AdminReview[] }>("/api/v1/admin/reviews"),

  approveReview: (deploymentId: string, opts?: { reason?: string }) =>
    request<{
      deployment_id: string;
      release_status: string;
      release_pending: boolean;
    }>(`/api/v1/admin/deployments/${deploymentId}/review/approve`, {
      method: "POST",
      body: JSON.stringify(opts ?? {}),
    }),

  rejectReview: (deploymentId: string, reason?: string) =>
    request<{ deployment_id: string; release_status: string }>(
      `/api/v1/admin/deployments/${deploymentId}/review/reject`,
      { method: "POST", body: JSON.stringify(reason ? { reason } : {}) },
    ),
};
