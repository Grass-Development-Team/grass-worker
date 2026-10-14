import { html } from "lit";
import { define } from "../base";
import { GwInput } from "../input";
import type { SelectOption } from "../select";
export class GwCombobox extends GwInput {
  static properties = { ...GwInput.properties, options: { attribute: false } };
  options: SelectOption[] = [];
  protected renderControl() {
    return html`<input
        id="control"
        role="combobox"
        list="options"
        autocomplete="off"
        .value=${this.value}
        ?required=${this.required}
        ?disabled=${this.isDisabled}
        aria-label=${this.label || this.name}
        placeholder=${this.placeholder}
        @input=${this.input}
        @change=${this.change}
        @keydown=${this.keydown}
      /><datalist id="options">
        ${this.options.filter((option) => !option.disabled).map((option) => html`<option value=${option.value}>${option.label}</option>`)}
      </datalist>`;
  }
}
define("gw-combobox", GwCombobox);
