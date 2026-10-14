import { QueryController } from "@/infra/query";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing, type PropertyValues } from "lit";
import { Page, field, formValues, heading, readForm, table, type FieldSpec } from "../base";
export type ResourceRow = { id: string } & Record<string, unknown>;
export function rows<T extends { id: string }>(items: T[]): ResourceRow[] {
  return items as unknown as ResourceRow[];
}
export interface ResourceDefinition {
  title: string;
  description: string;
  base: string;
  list: () => Promise<ResourceRow[]>;
  fields: (row: ResourceRow | null) => FieldSpec[];
  defaults?: Record<string, unknown>;
  columns: { label: string; key?: string; render?: (row: ResourceRow) => unknown }[];
  save: (values: Record<string, unknown>, row: ResourceRow | null) => Promise<unknown>;
  remove?: (row: ResourceRow) => Promise<unknown>;
}
export class ResourcePage extends Page {
  static properties = { ...Page.properties, filter: { state: true } };
  filter = "";
  protected definition!: ResourceDefinition;
  protected resource = new QueryController<ResourceRow[]>(this, {
    queryKey: ["admin", "unconfigured"],
    queryFn: async () => [],
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues): void {
    if (changed.has("context"))
      this.resource.configure({
        queryKey: ["admin", this.definition.base],
        queryFn: this.definition.list,
      });
  }
  protected extraEditor(_row: ResourceRow | null): unknown {
    return nothing;
  }
  protected extraList(): unknown {
    return nothing;
  }
  protected values(data: FormData, row: ResourceRow | null): Record<string, unknown> {
    return formValues(data, this.definition.fields(row));
  }
  protected editor(row: ResourceRow | null) {
    const fields = this.definition.fields(row);
    return html`<gw-card
      ><form
        class="form"
        @submit=${(event: Event) => {
          const data = readForm(event);
          void this.run(
            () => this.definition.save(this.values(data, row), row),
            async () => {
              await this.resource.refresh();
              this.go(this.definition.base);
            },
          );
        }}
      >
        <div class="grid">
          ${fields.map((spec) => field(spec, row?.[spec.name] ?? this.definition.defaults?.[spec.name]))}
        </div>
        ${this.extraEditor(row)}
        <div class="form-footer">
          <gw-button variant="outline" href=${this.definition.base}>Cancel</gw-button
          ><gw-button type="submit" .loading=${this.busy}
            >${row ? "Save changes" : "Create"}</gw-button
          >
        </div>
      </form></gw-card
    >`;
  }
  render() {
    this.classList.add("page");
    const definition = this.definition,
      state = this.queryState(this.resource);
    if (state) return state;
    const items = this.resource.result.data!,
      creating = this.context.path.endsWith("/new"),
      id = this.context.params.resourceId,
      row = items.find((item) => item.id === id) || null;
    if (creating || id)
      return html`${heading(row ? `Edit ${String(row.name || row.label || row.email || definition.title)}` : `New ${definition.title.toLowerCase()}`, definition.description, html`<gw-button variant="ghost" href=${definition.base}>Back to ${definition.title.toLowerCase()}</gw-button>`)}${this.error()}${id && !row ? html`<gw-alert tone="warning">This item could not be found.</gw-alert>` : this.editor(row)}`;
    const filtered = items.filter((item) =>
      JSON.stringify(item).toLowerCase().includes(this.filter.toLowerCase()),
    );
    return html`${heading(definition.title, definition.description, html`<gw-button href=${`${definition.base}/new`}>Create new</gw-button>`)}${this.error()}<gw-input
        label="Search"
        placeholder=${`Search ${definition.title.toLowerCase()}`}
        style="max-width:360px"
        .value=${this.filter}
        @gw-input=${(event: CustomEvent<{ value: string }>) => (this.filter = event.detail.value)}
      ></gw-input
      >${table(
        filtered,
        [
          ...definition.columns,
          {
            label: "Actions",
            render: (item) =>
              html`<div class="actions">
                <gw-button variant="ghost" size="sm" href=${`${definition.base}/${item.id}`}
                  >Edit</gw-button
                >${
                  definition.remove
                    ? html`<gw-button
                        variant="ghost"
                        size="sm"
                        ?disabled=${this.busy}
                        @click=${async () => {
                          if (
                            await confirmAction(
                              "Remove this item?",
                              "Review its dependencies before removing it.",
                              "Remove",
                            )
                          )
                            void this.run(
                              () => definition.remove!(item),
                              () => this.resource.refresh().then(() => undefined),
                              "Item removed.",
                            );
                        }}
                        >Remove</gw-button
                      >`
                    : nothing
                }
              </div>`,
          },
        ],
        definition.title,
      )}${this.extraList()}`;
  }
}
