import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwSeparator extends GwElement {
  static properties = { orientation: { reflect: true } };
  static styles = componentStyles(styles);
  orientation = "horizontal";
  render() {
    return html`<div role="separator" aria-orientation=${this.orientation}></div>`;
  }
}
define("gw-separator", GwSeparator);
