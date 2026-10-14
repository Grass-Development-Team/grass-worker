import { StoreController } from "@/infra/controllers/store";
import { Store } from "@/infra/state";
import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
interface Toast {
  id: number;
  message: string;
  tone: string;
}
const messages = new Store<Toast[]>([]);
let sequence = 0;
const timers = new Map<number, ReturnType<typeof setTimeout>>();
export function dismissToast(id: number): void {
  clearTimeout(timers.get(id));
  timers.delete(id);
  messages.set(messages.value.filter((item) => item.id !== id));
}
export function notify(message: string, tone = "success"): void {
  if (messages.value.some((item) => item.message === message && item.tone === tone)) return;
  const id = ++sequence;
  if (messages.value.length >= 4) dismissToast(messages.value[0].id);
  messages.set([...messages.value, { id, message, tone }]);
  timers.set(
    id,
    setTimeout(() => dismissToast(id), tone === "error" ? 8000 : 5000),
  );
}
export class GwToast extends GwElement {
  static styles = componentStyles(styles);
  private messages = new StoreController(this, messages);
  render() {
    return html`<div class="stack" aria-live="polite" aria-relevant="additions">
      ${this.messages.value.map((item) => html`<div class="toast" data-tone=${item.tone} role=${item.tone === "error" ? "alert" : "status"}><gw-icon name=${item.tone === "success" ? "check" : "activity"}></gw-icon><span>${item.message}</span><button type="button" aria-label="Dismiss notification" @click=${() => dismissToast(item.id)}>×</button></div>`)}
    </div>`;
  }
}
define("gw-toast", GwToast);
