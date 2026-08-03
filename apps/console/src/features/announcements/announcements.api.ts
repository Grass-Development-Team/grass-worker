import { request } from "@/lib/api";

export interface Announcement {
  id: string;
  title: string;
  content: string;
  auto_popup: boolean;
  published_at: string;
}

export interface AnnouncementsPage {
  announcements: Announcement[];
  pagination: {
    page: number;
    per_page: number;
    total: number;
    total_pages: number;
  };
}

export const announcementsApi = {
  list: (page = 1, perPage = 5) =>
    request<AnnouncementsPage>(`/api/v1/announcements?page=${page}&per_page=${perPage}`),
};
