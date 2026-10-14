import { html, nothing } from "lit";
import { componentStyles, define } from "../base";
import { GwInput } from "../input";
import inputStyles from "../input/style.css?inline";
import styles from "./style.css?inline";
export class GwPasswordInput extends GwInput {
  static properties = { ...GwInput.properties, visible: { type: Boolean } };
  static styles = componentStyles(inputStyles + styles);
  visible = false;
  constructor() {
    super();
    this.type = "password";
  }
  render() {
    return html`${this.label ? html`<label for="control">${this.label}${this.required ? " *" : ""}</label>` : nothing}
      <div class="split">
        <div class="control"><slot name="prefix"></slot>${super.renderControl()}</div>
        <button
          type="button"
          class="reveal"
          ?disabled=${this.isDisabled}
          aria-label=${this.visible ? "Hide password" : "Show password"}
          aria-pressed=${String(this.visible)}
          @click=${async () => {
            const control = this.control as HTMLInputElement;
            const start = control.selectionStart,
              end = control.selectionEnd;
            this.visible = !this.visible;
            this.type = this.visible ? "text" : "password";
            await this.updateComplete;
            this.focus();
            control.setSelectionRange(start, end);
          }}
        >
          <gw-icon name=${this.visible ? "eye-off" : "eye"}></gw-icon>
        </button>
      </div>
      ${this.error || this.description ? html`<span id="hint" class=${this.error ? "error" : "description"} role=${this.error ? "alert" : nothing}>${this.error || this.description}</span>` : nothing}`;
  }
}
define("gw-password-input", GwPasswordInput);
