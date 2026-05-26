import { request } from "@/lib/api";

export const authApi = {
  login: (email: string, password: string) =>
    request<{ user: { id: string; email: string } }>("/api/v1/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),
  logout: () => request<null>("/api/v1/auth/logout", { method: "POST" }),
  me: () => request<{ id: string; email: string }>("/api/v1/me"),
};
