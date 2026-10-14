import { GwElement, componentStyles, define } from "@/ui/components/base";
import { html } from "lit";
import styles from "./style.css?inline";
export class AuthLayout extends GwElement {
  static styles = componentStyles(styles);
  render() {
    return html`<main><slot></slot></main>
      <footer>Powered by Grass Worker</footer>`;
  }
}
define("gw-auth-layout", AuthLayout);
