import { getCsrfToken } from "@/lib/csrf";

export const API_UNAUTHORIZED_EVENT = "grass-worker:api-unauthorized";

interface ApiResponse<T> {
  code: number;
  message: string;
  data: T;
  op?: string;
}

interface RequestBehavior {
  broadcastUnauthorized?: boolean;
}

export class ApiError extends Error {
  readonly status: number;
  readonly code?: number;
  readonly operation?: string;

  constructor(message: string, status: number, code?: number, operation?: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.operation = operation;
  }
}

export function apiUrl(path: string): string {
  return `${baseUrl()}${path}`;
}

function baseUrl(): string {
  return (import.meta.env.VITE_API_BASE_URL ?? "").trim().replace(/\/+$/, "");
}

export async function request<T>(
  url: string,
  options?: RequestInit,
  behavior: RequestBehavior = {},
): Promise<T> {
  const headers: Record<string, string> = {};

  new Headers(options?.headers).forEach((value, key) => {
    headers[key] = value;
  });

  if (
    typeof options?.body === "string" &&
    !Object.keys(headers).some((key) => key.toLowerCase() === "content-type")
  ) {
    headers["Content-Type"] = "application/json";
  }

  if (options?.method && !["GET", "HEAD", "OPTIONS"].includes(options.method.toUpperCase())) {
    const csrf = getCsrfToken();
    if (csrf) {
      headers["x-csrf-token"] = csrf;
    }
  }

  const response = await fetch(apiUrl(url), {
    credentials: "include",
    ...options,
    headers,
  });

  const contentType = response.headers.get("content-type")?.toLowerCase() ?? "";
  let json: ApiResponse<T> | null = null;
  if (contentType.includes("application/json")) {
    try {
      json = (await response.json()) as ApiResponse<T>;
    } catch {
      throw new ApiError("Control API returned an invalid response", response.status);
    }
  }

  if (response.ok && json?.code === response.status) {
    return json.data;
  }

  if (
    response.status === 401 &&
    behavior.broadcastUnauthorized !== false &&
    typeof window !== "undefined"
  ) {
    window.dispatchEvent(new Event(API_UNAUTHORIZED_EVENT));
  }

  throw new ApiError(
    json?.message || `Request failed (${response.status})`,
    response.status,
    json?.code,
    json?.op,
  );
}
