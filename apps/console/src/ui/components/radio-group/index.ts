import { html, unsafeCSS } from "lit";
import { define } from "../base";
import { GwSelect } from "../select";
import styles from "./style.css?inline";
export class GwRadioGroup extends GwSelect {
  static styles = [...GwSelect.styles, unsafeCSS(styles)];
  render() {
    return html`<fieldset>
      <legend>${this.label}</legend>
      ${this.options.map(
        (option) =>
          html`<label
            ><input
              type="radio"
              name=${this.name || "option"}
              value=${option.value}
              .checked=${this.value === option.value}
              ?required=${this.required}
              ?disabled=${this.isDisabled || option.disabled}
              @change=${(event: Event) => {
                this.input(event);
                this.change();
              }}
            />${option.label}</label
          >`,
      )}
    </fieldset>`;
  }
}
define("gw-radio-group", GwRadioGroup);
