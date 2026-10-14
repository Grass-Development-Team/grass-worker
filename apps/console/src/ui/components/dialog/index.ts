import { html, type PropertyValues } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwDialog extends GwElement {
  static properties = { open: { type: Boolean, reflect: true }, heading: {}, description: {} };
  static styles = componentStyles(styles);
  open = false;
  heading = "";
  description = "";
  private previousFocus: HTMLElement | null = null;
  protected updated(changed: PropertyValues): void {
    if (!changed.has("open")) return;
    const dialog = this.renderRoot.querySelector("dialog")!;
    if (this.open && !dialog.open) {
      this.previousFocus = document.activeElement as HTMLElement;
      dialog.showModal();
    } else if (!this.open && dialog.open) {
      dialog.close();
      this.previousFocus?.focus();
    }
  }
  close(): void {
    this.open = false;
    this.dispatchEvent(new CustomEvent("gw-close", { bubbles: true, composed: true }));
  }
  disconnectedCallback(): void {
    this.renderRoot.querySelector("dialog")?.close();
    this.previousFocus?.focus();
    super.disconnectedCallback();
  }
  render() {
    return html`<dialog
      aria-labelledby="title"
      aria-describedby="description"
      @cancel=${(event: Event) => {
        event.preventDefault();
        this.close();
      }}
      @click=${(event: MouseEvent) => {
        if (event.target === event.currentTarget) {
          const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
          if (
            event.clientX < box.left ||
            event.clientX > box.right ||
            event.clientY < box.top ||
            event.clientY > box.bottom
          )
            this.close();
        }
      }}
    >
      <header>
        <div>
          <h2 id="title">${this.heading || "Dialog"}</h2>
          <p id="description">${this.description}</p>
        </div>
        <button type="button" aria-label="Close dialog" @click=${this.close}>×</button>
      </header>
      <div class="content"><slot></slot></div>
      <footer><slot name="footer"></slot></footer>
    </dialog>`;
  }
}
define("gw-dialog", GwDialog);
export class GwConfirmDialog extends GwDialog {}
define("gw-confirm-dialog", GwConfirmDialog);
export class GwSheet extends GwDialog {}
define("gw-sheet", GwSheet);
