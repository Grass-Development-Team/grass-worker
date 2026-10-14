export interface NotificationItem {
  id: string;
  action: string;
  announcement_id?: string | null;
  title: string;
  project: {
    id: string | null;
    name: string;
    slug: string;
  } | null;
  content: string | null;
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
