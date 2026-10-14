export interface ApiEnvelope<T> {
  code: number;
  message: string;
  data: T;
  op?: string;
}
export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly code?: number,
    readonly operation?: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}
export function errorMessage(cause: unknown): string {
  if (cause instanceof Error && cause.message.trim()) return cause.message;
  if (
    typeof cause === "object" &&
    cause !== null &&
    "message" in cause &&
    typeof cause.message === "string" &&
    cause.message.trim()
  )
    return cause.message;
  return "Something went wrong. Please try again.";
}
export function isEnvelope(value: unknown): value is ApiEnvelope<unknown> {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "number" &&
    "data" in value
  );
}
