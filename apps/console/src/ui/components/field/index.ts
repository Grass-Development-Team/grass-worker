import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwField extends GwElement {
  static properties = { label: { reflect: true } };
  static styles = componentStyles(styles);
  label = "";
  render() {
    return html`<fieldset>
      <legend>${this.label}</legend>
      <slot></slot>
    </fieldset>`;
  }
}
define("gw-field", GwField);
