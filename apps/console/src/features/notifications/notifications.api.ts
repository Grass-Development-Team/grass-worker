import { request } from "@/lib/api";

export interface NotificationItem {
  id: string;
  action: string;
  title: string;
  project: {
    id: string | null;
    name: string;
    slug: string;
  };
  actor: {
    id: string | null;
    label: string;
  };
  reason: string | null;
  target_url: string;
  read_at: string | null;
  created_at: string;
}

export interface NotificationsPage {
  notifications: NotificationItem[];
  pagination: {
    page: number;
    per_page: number;
    total: number;
    total_pages: number;
  };
}

export const notificationsApi = {
  list: (page = 1, perPage = 25) =>
    request<NotificationsPage>(`/api/v1/notifications?page=${page}&per_page=${perPage}`),

  unreadCount: () => request<{ count: number }>("/api/v1/notifications/unread-count"),

  markRead: (notificationId: string) =>
    request<{ ok: true }>(`/api/v1/notifications/${notificationId}/read`, {
      method: "POST",
    }),

  markAllRead: () =>
    request<{ updated: number }>("/api/v1/notifications/read-all", { method: "POST" }),
};
