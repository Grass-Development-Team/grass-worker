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
