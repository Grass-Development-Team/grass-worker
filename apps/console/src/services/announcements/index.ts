import { request } from "@/infra/api/index";
import type { AnnouncementsPage } from "./model";
export type * from "./model";

export const announcementsApi = {
  list: (page = 1, perPage = 5) =>
    request<AnnouncementsPage>(`/api/v1/announcements?page=${page}&per_page=${perPage}`),
};
