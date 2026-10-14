import { html, unsafeCSS } from "lit";
import { GwElement, define, sharedStyles } from "../base";
import styles from "./style.css?inline";
export class GwPagination extends GwElement {
  static properties = { page: { type: Number }, pages: { type: Number } };
  static styles = [sharedStyles, unsafeCSS(styles)];
  page = 1;
  pages = 1;
  private select(page: number): void {
    this.dispatchEvent(
      new CustomEvent("gw-page", { detail: { page }, bubbles: true, composed: true }),
    );
  }
  render() {
    return html`<nav aria-label="Pagination">
      <gw-button
        variant="outline"
        size="sm"
        ?disabled=${this.page <= 1}
        @click=${() => this.select(this.page - 1)}
        >Previous</gw-button
      ><span>Page ${this.page} of ${Math.max(1, this.pages)}</span
      ><gw-button
        variant="outline"
        size="sm"
        ?disabled=${this.page >= this.pages}
        @click=${() => this.select(this.page + 1)}
        >Next</gw-button
      >
    </nav>`;
  }
}
define("gw-pagination", GwPagination);
