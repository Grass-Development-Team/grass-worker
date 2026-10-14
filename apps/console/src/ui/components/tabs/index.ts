import { html, unsafeCSS } from "lit";
import { GwElement, define, sharedStyles } from "../base";
import styles from "./style.css?inline";
export class GwTabs extends GwElement {
  static properties = { tabs: { attribute: false }, value: {} };
  static styles = [sharedStyles, unsafeCSS(styles)];
  tabs: { value: string; label: string }[] = [];
  value = "";
  private select(value: string): void {
    this.value = value;
    this.dispatchEvent(
      new CustomEvent("gw-change", { detail: { value }, bubbles: true, composed: true }),
    );
  }
  render() {
    const selected = this.value || this.tabs[0]?.value;
    return html`<div class="list" role="tablist">
        ${this.tabs.map(
          (tab, index) =>
            html`<button
              id=${`tab-${tab.value}`}
              role="tab"
              aria-selected=${String(selected === tab.value)}
              aria-controls=${`panel-${tab.value}`}
              tabindex=${selected === tab.value ? 0 : -1}
              @click=${() => this.select(tab.value)}
              @keydown=${(event: KeyboardEvent) => {
                const next =
                  event.key === "ArrowRight"
                    ? (index + 1) % this.tabs.length
                    : event.key === "ArrowLeft"
                      ? (index - 1 + this.tabs.length) % this.tabs.length
                      : event.key === "Home"
                        ? 0
                        : event.key === "End"
                          ? this.tabs.length - 1
                          : -1;
                if (next >= 0) {
                  event.preventDefault();
                  this.select(this.tabs[next].value);
                  void this.updateComplete.then(() =>
                    this.renderRoot
                      .querySelector<HTMLElement>(`#tab-${this.tabs[next].value}`)
                      ?.focus(),
                  );
                }
              }}
            >
              ${tab.label}
            </button>`,
        )}
      </div>
      ${this.tabs.map((tab) => html`<div id=${`panel-${tab.value}`} role="tabpanel" aria-labelledby=${`tab-${tab.value}`} ?hidden=${selected !== tab.value}><slot name=${tab.value}></slot></div>`)}`;
  }
}
define("gw-tabs", GwTabs);
