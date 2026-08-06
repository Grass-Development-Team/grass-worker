import { toast } from "sonner";

import { showErrorToast } from "@/lib/toast";

import type { AdminBatchItemResult } from "./admin.api";

export function showBatchResultToast(
  results: AdminBatchItemResult[],
  itemLabel: string,
  successVerb: string,
): void {
  const succeeded = results.filter((result) => result.success).length;
  const failures = results.filter((result) => !result.success);

  if (failures.length === 0) {
    toast.success(`${succeeded} ${itemLabel} ${successVerb}.`);
    return;
  }

  const firstMessage = failures.find((result) => result.message)?.message;
  const summary = `${succeeded} succeeded; ${failures.length} failed${firstMessage ? `: ${firstMessage}` : "."}`;
  showErrorToast(new Error(summary));
}
