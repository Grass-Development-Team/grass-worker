import { ApiError, request } from "./client";

export const currentUserQueryKey = ["current-user"] as const;

export type CurrentUser = {
  id: string;
  email: string;
  is_admin: boolean;
  is_initial_admin: boolean;
};

type UserEnvelope = {
  user: CurrentUser;
};

export async function login(
  email: string,
  password: string,
): Promise<CurrentUser> {
  const response = await request<UserEnvelope>("/api/v1/auth/login", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  });

  return response.user;
}

export async function getCurrentUser(): Promise<CurrentUser | null> {
  try {
    const response = await request<UserEnvelope>("/api/v1/me");
    return response.user;
  } catch (error) {
    if (error instanceof ApiError && error.status === 401) {
      return null;
    }

    throw error;
  }
}

export async function logout(): Promise<void> {
  await request<void>("/api/v1/auth/logout", { method: "POST" });
}
