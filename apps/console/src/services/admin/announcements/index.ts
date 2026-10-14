import { request } from "@/infra/api/index";
import type { AdminAnnouncement, AdminAnnouncementsPage } from "./model";
export const announcementsApi = {
  listAnnouncements: (page = 1, perPage = 20) =>
    request<AdminAnnouncementsPage>(`/api/v1/admin/announcements?page=${page}&per_page=${perPage}`),

  publishAnnouncement: (input: { title: string; content: string; auto_popup: boolean }) =>
    request<{ announcement: AdminAnnouncement; recipients: number }>(
      "/api/v1/admin/announcements",
      {
        method: "POST",
        body: JSON.stringify(input),
      },
    ),

  removeAnnouncement: (announcementId: string) =>
    request<{ ok: true }>(`/api/v1/admin/announcements/${announcementId}`, {
      method: "DELETE",
    }),
};
