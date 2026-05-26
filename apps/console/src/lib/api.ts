interface ApiResponse<T> {
  code: number;
  message: string;
  data: T;
}

export async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await fetch(url, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  const json: ApiResponse<T> = await response.json();
  if (response.ok && json.code === 200) {
    return json.data;
  }
  throw new Error(json.message || "Request failed");
}
