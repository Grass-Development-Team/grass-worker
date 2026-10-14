import { request } from "@/infra/api/index";
import type { NotificationItem, NotificationsPage } from "./model";
export type * from "./model";

export const notificationsApi = {
  list: (page = 1, perPage = 25) =>
    request<NotificationsPage>(`/api/v1/notifications?page=${page}&per_page=${perPage}`),

  unreadCount: () => request<{ count: number }>("/api/v1/notifications/unread-count"),

  autoPopup: () =>
    request<{ notification: NotificationItem | null }>("/api/v1/notifications/auto-popup"),

  markRead: (notificationId: string) =>
    request<{ ok: true }>(`/api/v1/notifications/${notificationId}/read`, {
      method: "POST",
    }),

  markAllRead: () =>
    request<{ updated: number }>("/api/v1/notifications/read-all", { method: "POST" }),
};
