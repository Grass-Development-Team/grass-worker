import { html, nothing } from "lit";
import { define } from "../base";
import { GwInput } from "../input";
export class GwTextarea extends GwInput {
  constructor() {
    super();
    this.type = "textarea";
  }
  protected renderControl() {
    return html`<textarea
      id="control"
      .value=${this.value}
      name=${this.name}
      placeholder=${this.placeholder}
      ?required=${this.required}
      ?disabled=${this.isDisabled}
      ?readonly=${this.readonly}
      minlength=${this.minLength >= 0 ? this.minLength : nothing}
      maxlength=${this.maxLength >= 0 ? this.maxLength : nothing}
      aria-label=${this.label || this.name}
      aria-describedby=${this.error || this.description ? "hint" : nothing}
      aria-invalid=${String(Boolean(this.error))}
      @input=${this.input}
      @change=${this.change}
    ></textarea>`;
  }
}
define("gw-textarea", GwTextarea);
