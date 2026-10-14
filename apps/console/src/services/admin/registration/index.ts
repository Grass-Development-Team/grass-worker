import { request } from "@/infra/api/index";
import type { AdminRegistrationEmail } from "./model";
export const registrationApi = {
  listRegistrationEmails: () =>
    request<{ emails: AdminRegistrationEmail[] }>("/api/v1/admin/registration/emails"),

  addRegistrationEmail: (input: { email: string }) =>
    request<{ email: AdminRegistrationEmail }>("/api/v1/admin/registration/emails", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  removeRegistrationEmail: (entryId: string) =>
    request<{ deleted: boolean }>(`/api/v1/admin/registration/emails/${entryId}`, {
      method: "DELETE",
    }),
};
