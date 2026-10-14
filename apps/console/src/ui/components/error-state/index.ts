import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwErrorState extends GwElement {
  static properties = { message: { reflect: true } };
  static styles = componentStyles(styles);
  message = "Unable to load this page.";
  render() {
    return html`<section role="alert">
      <h2>Something went wrong</h2>
      <p>${this.message}</p>
      <slot></slot>
    </section>`;
  }
}
define("gw-error-state", GwErrorState);
