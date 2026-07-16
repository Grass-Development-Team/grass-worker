import { request } from "@/lib/api";
import { setCsrfToken } from "@/lib/csrf";

interface AuthResponse {
  user: { id: string; email: string; display_name: string | null };
  csrf_token: string;
}

export interface RegisterInput {
  email: string;
  display_name: string;
  password: string;
  invitation_token?: string;
}

interface MeResponse {
  user: { id: string; email: string; display_name: string | null };
}

interface CsrfResponse {
  csrf_token: string;
}

export const authApi = {
  login: async (email: string, password: string) => {
    const data = await request<AuthResponse>("/api/v1/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
      credentials: "include" as RequestCredentials,
    });
    setCsrfToken(data.csrf_token);
    return data;
  },
  register: async (input: RegisterInput) => {
    const data = await request<AuthResponse>("/api/v1/auth/register", {
      method: "POST",
      body: JSON.stringify(input),
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
  csrf: async () => {
    const data = await request<CsrfResponse>("/api/v1/auth/csrf", {
      credentials: "include" as RequestCredentials,
    });
    setCsrfToken(data.csrf_token);
    return data;
  },
  restore: async () => {
    const data = await authApi.me();
    await authApi.csrf();
    return data;
  },
};
