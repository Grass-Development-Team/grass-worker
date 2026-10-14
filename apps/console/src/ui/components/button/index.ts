import { html, nothing } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwButton extends GwElement {
  static properties = {
    variant: { reflect: true },
    size: { reflect: true },
    disabled: { type: Boolean, reflect: true },
    loading: { type: Boolean },
    type: {},
    href: {},
  };
  static styles = componentStyles(styles);
  variant = "primary";
  size = "md";
  disabled = false;
  loading = false;
  type = "button";
  href = "";
  focus(options?: FocusOptions): void {
    this.renderRoot.querySelector<HTMLElement>("button, a")?.focus(options);
  }
  private activate(event: MouseEvent): void {
    if (this.disabled || this.loading) {
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }
    const form = this.closest("form");
    if (this.type === "submit" && form) {
      event.preventDefault();
      form.requestSubmit();
    }
    if (this.type === "reset" && form) {
      event.preventDefault();
      form.reset();
    }
  }
  render() {
    const content = html`${this.loading ? html`<span class="spinner" aria-hidden="true"></span>` : html`<slot name="icon"></slot>`}<slot
      ></slot>`;
    return this.href
      ? html`<a
          href=${this.href}
          aria-label=${this.getAttribute("aria-label") || nothing}
          aria-disabled=${String(this.disabled)}
          @click=${this.activate}
          >${content}</a
        >`
      : html`<button
          type="button"
          aria-label=${this.getAttribute("aria-label") || nothing}
          ?disabled=${this.disabled || this.loading}
          aria-busy=${String(this.loading)}
          @click=${this.activate}
        >
          ${content}
        </button>`;
  }
}
define("gw-button", GwButton);
