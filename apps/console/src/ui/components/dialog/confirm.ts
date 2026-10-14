import { location } from "@/infra/router";
import { GwConfirmDialog } from ".";
import type { GwButton } from "../button";
export function confirmAction(
  heading: string,
  description: string,
  action = "Confirm",
): Promise<boolean> {
  return new Promise((resolve) => {
    const dialog = document.createElement("gw-confirm-dialog") as GwConfirmDialog;
    dialog.heading = heading;
    dialog.description = description;
    const cancel = document.createElement("gw-button") as GwButton;
    cancel.slot = "footer";
    cancel.variant = "outline";
    cancel.textContent = "Cancel";
    const confirm = document.createElement("gw-button") as GwButton;
    confirm.slot = "footer";
    confirm.variant = "danger";
    confirm.textContent = action;
    const finish = (value: boolean) => {
      unsubscribe();
      dialog.open = false;
      dialog.remove();
      resolve(value);
    };
    const unsubscribe = location.subscribe(() => finish(false));
    cancel.addEventListener("click", () => finish(false));
    confirm.addEventListener("click", () => finish(true));
    dialog.addEventListener("gw-close", () => finish(false));
    dialog.append(cancel, confirm);
    document.body.append(dialog);
    dialog.open = true;
  });
}
