import { html, type PropertyValues } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwPopover extends GwElement {
  static properties = { label: {}, open: { type: Boolean, reflect: true } };
  static styles = componentStyles(styles);
  label = "Open menu";
  open = false;
  private get panel() {
    return this.renderRoot.querySelector<HTMLElement>(".panel")!;
  }
  protected updated(changed: PropertyValues): void {
    if (!changed.has("open")) return;
    if (this.open && !this.panel.matches(":popover-open")) this.panel.showPopover();
    else if (!this.open && this.panel.matches(":popover-open")) this.panel.hidePopover();
  }
  private toggle(): void {
    this.open = !this.open;
  }
  private position(): void {
    const panel = this.panel,
      anchor = this.getBoundingClientRect();
    panel.style.top = `${Math.max(12, Math.min(anchor.bottom + 8, innerHeight - panel.offsetHeight - 12))}px`;
    panel.style.left = `${Math.max(12, Math.min(anchor.left, innerWidth - panel.offsetWidth - 12))}px`;
  }
  private keydown(event: KeyboardEvent): void {
    const items = [
      ...this.querySelectorAll<HTMLElement>('[role="menuitem"], a, button, gw-button'),
    ].filter((item) => !item.hasAttribute("disabled"));
    let active: Element | null = document.activeElement;
    while (active?.shadowRoot?.activeElement) active = active.shadowRoot.activeElement;
    const current = items.findIndex(
      (item) =>
        item === active ||
        item.contains(active) ||
        Boolean(active && item.shadowRoot?.contains(active)),
    );
    const next =
      event.key === "ArrowDown"
        ? (current + 1) % items.length
        : event.key === "ArrowUp"
          ? (current - 1 + items.length) % items.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? items.length - 1
              : -1;
    if (next >= 0 && items.length) {
      event.preventDefault();
      items[next].focus();
    }
  }
  render() {
    const menu = this.localName === "gw-dropdown-menu";
    return html`<button
        class="trigger"
        type="button"
        aria-label=${this.label}
        aria-expanded=${String(this.open)}
        aria-haspopup=${menu ? "menu" : "dialog"}
        @click=${this.toggle}
      >
        <slot name="trigger">${this.label}</slot>
      </button>
      <div
        class="panel"
        popover="auto"
        role=${menu ? "menu" : "dialog"}
        aria-label=${this.label}
        @keydown=${this.keydown}
        @click=${(event: Event) => {
          if (
            menu &&
            event
              .composedPath()
              .some(
                (item) => item instanceof HTMLElement && item.getAttribute("role") === "menuitem",
              )
          )
            this.open = false;
        }}
        @toggle=${(event: ToggleEvent) => {
          this.open = event.newState === "open";
          if (this.open) this.position();
        }}
      >
        <slot></slot>
      </div>`;
  }
}
define("gw-popover", GwPopover);
export class GwDropdownMenu extends GwPopover {}
define("gw-dropdown-menu", GwDropdownMenu);
