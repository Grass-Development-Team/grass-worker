import { getCsrfRevision, getCsrfToken } from "@/infra/api/csrf";

export const API_UNAUTHORIZED_EVENT = "grass-worker:api-unauthorized";

import { ApiError, isEnvelope, type ApiEnvelope } from "@/infra/serializer";
export { ApiError } from "@/infra/serializer";
interface RequestBehavior {
  broadcastUnauthorized?: boolean;
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
  const authenticationRevision = getCsrfRevision();
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
  let json: ApiEnvelope<T> | null = null;
  if (contentType.includes("application/json")) {
    try {
      const value: unknown = await response.json();
      if (!isEnvelope(value)) throw new Error("Invalid envelope");
      json = value as ApiEnvelope<T>;
    } catch {
      throw new ApiError("Control API returned an invalid response", response.status);
    }
  }

  if (response.ok && json?.code === response.status) {
    return json.data;
  }

  if (
    response.status === 401 &&
    authenticationRevision === getCsrfRevision() &&
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
