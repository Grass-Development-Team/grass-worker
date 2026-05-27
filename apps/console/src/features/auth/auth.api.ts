import { request } from "@/lib/api";
import { setCsrfToken } from "@/lib/csrf";

interface LoginResponse {
  user: { id: string; email: string; display_name: string | null };
  csrf_token: string;
}

interface MeResponse {
  user: { id: string; email: string; display_name: string | null };
}

export const authApi = {
  login: async (email: string, password: string) => {
    const data = await request<LoginResponse>("/api/v1/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
      credentials: "include" as RequestCredentials,
    });
    setCsrfToken(data.csrf_token);
    return data;
  },
  logout: async () => {
    const data = await request<{ message: string }>("/api/v1/auth/logout", {
      method: "POST",
      credentials: "include" as RequestCredentials,
    });
    setCsrfToken(null);
    return data;
  },
  me: () =>
    request<MeResponse>("/api/v1/me", {
      credentials: "include" as RequestCredentials,
    }),
};
