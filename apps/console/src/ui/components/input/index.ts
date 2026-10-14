import { html, nothing, type PropertyValues } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwInput extends GwElement {
  static formAssociated = true;
  static properties = {
    value: {},
    name: { reflect: true },
    label: {},
    description: {},
    error: {},
    type: {},
    placeholder: {},
    autocomplete: {},
    inputmode: {},
    required: { type: Boolean },
    disabled: { type: Boolean, reflect: true },
    readonly: { type: Boolean },
    minLength: { type: Number, attribute: "minlength" },
    maxLength: { type: Number, attribute: "maxlength" },
    min: {},
    max: {},
    step: {},
    pattern: {},
    size: { reflect: true },
  };
  static styles = componentStyles(styles);
  value = "";
  name = "";
  label = "";
  description = "";
  error = "";
  type = "text";
  placeholder = "";
  autocomplete = "";
  inputmode = "";
  required = false;
  disabled = false;
  readonly = false;
  minLength = -1;
  maxLength = -1;
  min = "";
  max = "";
  step = "";
  pattern = "";
  size = "md";
  protected internals = this.attachInternals();
  private initialValue = "";
  private fieldsetDisabled = false;
  protected get control() {
    return this.renderRoot.querySelector<
      HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
    >("input, textarea, select");
  }
  get form() {
    return this.internals.form;
  }
  focus(options?: FocusOptions): void {
    this.control?.focus(options);
  }
  checkValidity(): boolean {
    return this.internals.checkValidity();
  }
  reportValidity(): boolean {
    return this.internals.reportValidity();
  }
  formResetCallback(): void {
    this.value = this.initialValue;
    this.error = "";
  }
  formStateRestoreCallback(state: string): void {
    this.value = state;
  }
  formDisabledCallback(disabled: boolean): void {
    this.fieldsetDisabled = disabled;
    this.requestUpdate();
  }
  protected get isDisabled() {
    return this.disabled || this.fieldsetDisabled;
  }
  protected firstUpdated(): void {
    this.initialValue = this.value;
    this.addEventListener("invalid", () => this.focus());
  }
  protected updated(_changed: PropertyValues): void {
    this.sync();
  }
  protected sync(): void {
    const control = this.control;
    if (!control) return;
    this.internals.setFormValue(this.isDisabled ? null : this.value);
    this.internals.setValidity(
      this.error ? { customError: true } : control.validity,
      this.error || control.validationMessage,
      control,
    );
  }
  protected input(event: Event): void {
    this.value = (event.target as HTMLInputElement).value;
    this.error = "";
    this.sync();
    this.dispatchEvent(
      new CustomEvent("gw-input", { detail: { value: this.value }, bubbles: true, composed: true }),
    );
  }
  protected change(): void {
    this.dispatchEvent(
      new CustomEvent("gw-change", {
        detail: { value: this.value },
        bubbles: true,
        composed: true,
      }),
    );
  }
  protected keydown(event: KeyboardEvent): void {
    if (event.key === "Enter" && !event.isComposing && this.type !== "textarea") {
      event.preventDefault();
      this.form?.requestSubmit();
    }
  }
  protected renderControl() {
    return html`<input
      id="control"
      .value=${this.value}
      type=${this.type}
      name=${this.name}
      placeholder=${this.placeholder}
      autocomplete=${this.autocomplete || nothing}
      inputmode=${this.inputmode || nothing}
      ?required=${this.required}
      ?disabled=${this.isDisabled}
      ?readonly=${this.readonly}
      minlength=${this.minLength >= 0 ? this.minLength : nothing}
      maxlength=${this.maxLength >= 0 ? this.maxLength : nothing}
      min=${this.min || nothing}
      max=${this.max || nothing}
      step=${this.step || nothing}
      pattern=${this.pattern || nothing}
      aria-label=${this.label || this.getAttribute("aria-label") || this.name}
      aria-describedby=${this.error || this.description ? "hint" : nothing}
      aria-invalid=${String(Boolean(this.error))}
      @input=${this.input}
      @change=${this.change}
      @keydown=${this.keydown}
    />`;
  }
  render() {
    return html`${this.label ? html`<label for="control">${this.label}${this.required ? html`<span aria-hidden="true"> *</span>` : nothing}</label>` : nothing}
      <div class="control" part="control">
        <slot name="prefix"></slot>${this.renderControl()}<slot name="suffix"></slot>
      </div>
      ${this.error || this.description ? html`<span id="hint" class=${this.error ? "error" : "description"} role=${this.error ? "alert" : nothing}>${this.error || this.description}</span>` : nothing}`;
  }
}
define("gw-input", GwInput);
