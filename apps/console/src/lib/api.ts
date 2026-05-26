interface ApiResponse<T> {
  code: number;
  message: string;
  data: T;
}

function baseUrl(): string {
  return import.meta.env.VITE_API_BASE_URL ?? "";
}

export async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await fetch(`${baseUrl()}${url}`, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  const json: ApiResponse<T> = await response.json();
  if (response.ok && json.code === 200) {
    return json.data;
  }
  throw new Error(json.message || "Request failed");
}
