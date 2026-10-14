import { html, nothing } from "lit";
import { define } from "../base";
import { GwInput } from "../input";
export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}
export class GwSelect extends GwInput {
  static properties = { ...GwInput.properties, options: { attribute: false } };
  options: SelectOption[] = [];
  protected renderControl() {
    return html`<select
      id="control"
      name=${this.name}
      ?required=${this.required}
      ?disabled=${this.isDisabled}
      aria-label=${this.label || this.name}
      aria-describedby=${this.error || this.description ? "hint" : nothing}
      @change=${(event: Event) => {
        this.input(event);
        this.change();
      }}
    >
      ${this.options.map((option) => html`<option value=${option.value} ?selected=${option.value === this.value} ?disabled=${option.disabled}>${option.label}</option>`)}
    </select>`;
  }
}
define("gw-select", GwSelect);
