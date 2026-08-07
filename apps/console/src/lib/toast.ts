import { toast } from "sonner";

const UNKNOWN_ERROR_MESSAGE = "Something went wrong.";

function normalizeErrorMessage(message: string): string {
  return message.trim() ? message : UNKNOWN_ERROR_MESSAGE;
}

export function getErrorMessage(cause: unknown): string {
  if (cause instanceof Error) return normalizeErrorMessage(cause.message);

  if (
    typeof cause === "object" &&
    cause !== null &&
    "message" in cause &&
    typeof cause.message === "string"
  ) {
    return normalizeErrorMessage(cause.message);
  }

  return UNKNOWN_ERROR_MESSAGE;
}

export function showErrorToast(cause: unknown, id?: string | number): void {
  const message = getErrorMessage(cause);
  if (id !== undefined) {
    toast.error(message, { id });
    return;
  }

  toast.error(message);
}
