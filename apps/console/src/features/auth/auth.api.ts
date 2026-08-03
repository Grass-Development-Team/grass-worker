import { request } from "@/lib/api";
import { setCsrfToken } from "@/lib/csrf";

export type PlatformRole = "admin" | "user";

export interface AuthUser {
  id: string;
  email: string;
  display_name: string | null;
  platform_role: PlatformRole;
}

interface AuthResponse {
  user: AuthUser;
  csrf_token: string;
}

export interface RegisterInput {
  email: string;
  display_name: string;
  password: string;
  invitation_token?: string;
}

interface MeResponse {
  user: AuthUser;
}

interface CsrfResponse {
  csrf_token: string;
}

let restorePromise: Promise<MeResponse> | null = null;

function restoreSession(): Promise<MeResponse> {
  if (!restorePromise) {
    const pending = (async () => {
      const data = await authApi.me();
      await authApi.csrf();
      return data;
    })();
    const shared = pending.finally(() => {
      if (restorePromise === shared) restorePromise = null;
    });
    restorePromise = shared;
  }
  return restorePromise;
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
  updateMe: (input: { display_name: string | null }) =>
    request<MeResponse>("/api/v1/me", {
      method: "PATCH",
      body: JSON.stringify(input),
    }),
  csrf: async () => {
    const data = await request<CsrfResponse>("/api/v1/auth/csrf", {
      credentials: "include" as RequestCredentials,
    });
    setCsrfToken(data.csrf_token);
    return data;
  },
  restore: restoreSession,
};
