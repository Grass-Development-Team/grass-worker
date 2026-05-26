import { getCsrfToken } from "@/lib/csrf";

interface ApiResponse<T> {
  code: number;
  message: string;
  data: T;
}

function baseUrl(): string {
  return import.meta.env.VITE_API_BASE_URL ?? "";
}

export async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };

  if (options?.method && !["GET", "HEAD", "OPTIONS"].includes(options.method)) {
    const csrf = getCsrfToken();
    if (csrf) {
      headers["x-csrf-token"] = csrf;
    }
  }

  const response = await fetch(`${baseUrl()}${url}`, {
    ...options,
    headers: {
      ...headers,
      ...(options?.headers as Record<string, string> | undefined),
    },
  });
  const json: ApiResponse<T> = await response.json();
  if (response.ok && json.code === 200) {
    return json.data;
  }
  throw new Error(json.message || "Request failed");
}
