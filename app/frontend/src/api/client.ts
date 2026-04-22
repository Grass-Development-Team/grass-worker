export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

type ErrorEnvelope = {
  error?: string;
};

export async function request<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
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
