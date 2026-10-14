import { html, unsafeCSS, type PropertyValues } from "lit";
import { define } from "../base";
import { GwInput } from "../input";
import styles from "./style.css?inline";
export class GwCheckbox extends GwInput {
  static properties = { ...GwInput.properties, checked: { type: Boolean, reflect: true } };
  static styles = [...GwInput.styles, unsafeCSS(styles)];
  checked = false;
  private initialChecked = false;
  protected firstUpdated(): void {
    super.firstUpdated();
    this.initialChecked = this.checked;
  }
  formResetCallback(): void {
    this.checked = this.initialChecked;
  }
  formStateRestoreCallback(state: string): void {
    this.checked = state === "on";
  }
  protected updated(_changed: PropertyValues): void {
    const control = this.control as HTMLInputElement;
    if (!control) return;
    this.internals.setFormValue(this.checked && !this.isDisabled ? this.value || "on" : null);
    this.internals.setValidity(
      this.required && !this.checked ? { valueMissing: true } : {},
      this.required && !this.checked ? "Please select this option." : "",
      control,
    );
  }
  render() {
    return html`<label
        ><input
          class=${this.localName === "gw-switch" ? "switch" : ""}
          type="checkbox"
          role=${this.localName === "gw-switch" ? "switch" : "checkbox"}
          .checked=${this.checked}
          ?disabled=${this.isDisabled}
          ?required=${this.required}
          aria-label=${this.label}
          @change=${(event: Event) => {
            this.checked = (event.target as HTMLInputElement).checked;
            this.dispatchEvent(
              new CustomEvent("gw-change", {
                detail: { checked: this.checked, value: this.checked },
                bubbles: true,
                composed: true,
              }),
            );
          }} /><span>${this.label}<slot></slot></span></label
      >${this.description ? html`<span class="description">${this.description}</span>` : ""}`;
  }
}
define("gw-checkbox", GwCheckbox);
export class GwSwitch extends GwCheckbox {}
define("gw-switch", GwSwitch);
