import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export interface TableColumn {
  label: string;
  key?: string;
  render?: (row: unknown) => unknown;
}
export class GwTable extends GwElement {
  static properties = {
    rows: { attribute: false },
    columns: { attribute: false },
    caption: {},
    empty: {},
  };
  static styles = componentStyles(styles);
  rows: unknown[] = [];
  columns: TableColumn[] = [];
  caption = "Results";
  empty = "No results.";
  render() {
    return html`<div class="scroll" tabindex="0" role="region" aria-label=${this.caption}>
      <table>
        <caption class="sr-only">
          ${this.caption}
        </caption>
        <thead>
          <tr>
            ${this.columns.map((column) => html`<th scope="col">${column.label}</th>`)}
          </tr>
        </thead>
        <tbody>
          ${
            this.rows.length
              ? this.rows.map(
                  (row) =>
                    html`<tr>
                      ${this.columns.map((column) => html`<td>${column.render ? column.render(row) : String((row as Record<string, unknown>)[column.key ?? ""] ?? "—")}</td>`)}
                    </tr>`,
                )
              : html`<tr>
                  <td colspan=${this.columns.length} class="empty">${this.empty}</td>
                </tr>`
          }
        </tbody>
      </table>
    </div>`;
  }
}
define("gw-table", GwTable);
