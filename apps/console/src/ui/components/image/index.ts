import { html, unsafeCSS } from "lit";
import { GwElement, define, sharedStyles } from "../base";
import styles from "./style.css?inline";
export class GwImage extends GwElement {
  static properties = { src: {}, alt: {}, failed: { state: true } };
  static styles = [sharedStyles, unsafeCSS(styles)];
  src = "";
  alt = "";
  failed = false;
  protected willUpdate(changed: Map<string, unknown>): void {
    if (changed.has("src")) this.failed = false;
  }
  render() {
    return this.src && !this.failed
      ? html`<img
          src=${this.src}
          alt=${this.alt}
          loading="lazy"
          @error=${() => (this.failed = true)}
        />`
      : html`<span class="fallback" role="img" aria-label=${this.alt}
          ><slot name="fallback">${this.alt || "Image unavailable"}</slot></span
        >`;
  }
}
define("gw-image", GwImage);
