export const currentUserQueryKey = ["current-user"] as const;

export type CurrentUser = {
  id: string;
  email: string;
  is_admin: boolean;
  is_initial_admin: boolean;
};

type ErrorEnvelope = {
  error?: string;
};

type UserEnvelope = {
  user: CurrentUser;
};

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);

  if (init.body && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }

  const response = await fetch(path, {
    credentials: "same-origin",
    ...init,
    headers,
  });

  if (response.status === 204) {
    return undefined as T;
  }

  const text = await response.text();
  const json = text ? (JSON.parse(text) as T | ErrorEnvelope) : null;

  if (!response.ok) {
    throw new ApiError(
      response.status,
      (json as ErrorEnvelope | null)?.error ?? "Request failed",
    );
  }

  return json as T;
}

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
