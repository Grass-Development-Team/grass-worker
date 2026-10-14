import { html, unsafeCSS } from "lit";
import { define } from "../base";
import { GwImage } from "../image";
import styles from "./style.css?inline";
export class GwAvatar extends GwImage {
  static styles = [...GwImage.styles, unsafeCSS(styles)];
  render() {
    return this.src && !this.failed
      ? super.render()
      : html`<span class="fallback" role="img" aria-label=${this.alt}
          >${this.alt.trim().slice(0, 2).toUpperCase() || "GW"}</span
        >`;
  }
}
define("gw-avatar", GwAvatar);
