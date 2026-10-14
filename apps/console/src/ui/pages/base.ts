import { QueryController } from "@/infra/query";
import { navigate } from "@/infra/router";
import { errorMessage } from "@/infra/serializer";
import { notify } from "@/ui/components/toast";
import { LitElement, html, nothing, type TemplateResult } from "lit";
export interface PageContext {
  params: Record<string, string>;
  path: string;
  search: URLSearchParams;
}
export class Page extends LitElement {
  static properties = {
    context: { attribute: false },
    busy: { state: true },
    failure: { state: true },
  };
  context: PageContext = { params: {}, path: "/", search: new URLSearchParams() };
  busy = false;
  failure = "";
  protected go(path: string, replace = false): void {
    if (this.isConnected) navigate(path, replace);
  }
  protected createRenderRoot() {
    return this;
  }
  protected async run<T>(
    operation: () => Promise<T>,
    onSuccess?: (result: T) => void | Promise<void>,
    message = "Saved successfully.",
  ): Promise<T | undefined> {
    if (this.busy || !this.isConnected) return;
    this.busy = true;
    this.failure = "";
    try {
      const result = await operation();
      if (!this.isConnected) return undefined;
      await onSuccess?.(result);
      if (message) notify(message);
      return result;
    } catch (cause) {
      this.failure = errorMessage(cause);
      notify(this.failure, "error");
      return undefined;
    } finally {
      this.busy = false;
    }
  }
  protected queryState<T>(query: QueryController<T>): TemplateResult<1> | undefined {
    if (query.result.isPending)
      return html`<div class="loading" role="status">
        <gw-spinner></gw-spinner><span>Loading…</span>
      </div>`;
    if (query.result.isError)
      return html`<gw-error-state .message=${errorMessage(query.result.error)}
        ><gw-button variant="outline" @click=${() => query.refresh()}
          >Try again</gw-button
        ></gw-error-state
      >`;
    return undefined;
  }
  protected error() {
    return this.failure ? html`<gw-alert tone="error">${this.failure}</gw-alert>` : nothing;
  }
}
export interface FieldSpec {
  name: string;
  label: string;
  type?: string;
  required?: boolean;
  description?: string;
  placeholder?: string;
  options?: { value: string; label: string; disabled?: boolean }[];
  min?: string;
  max?: string;
  step?: string;
  autocomplete?: string;
  readonly?: boolean;
}
export function field(spec: FieldSpec, value: unknown = "") {
  if (spec.type === "boolean")
    return html`<gw-switch
      name=${spec.name}
      label=${spec.label}
      description=${spec.description || ""}
      .checked=${Boolean(value)}
    ></gw-switch>`;
  if (spec.options)
    return html`<gw-select
      name=${spec.name}
      label=${spec.label}
      .value=${String(value ?? "")}
      .options=${spec.options}
      ?required=${spec.required}
      description=${spec.description || ""}
    ></gw-select>`;
  if (spec.type === "textarea" || spec.type === "json")
    return html`<gw-textarea
      name=${spec.name}
      label=${spec.label}
      .value=${spec.type === "json" ? JSON.stringify(value ?? {}, null, 2) : String(value ?? "")}
      ?required=${spec.required}
      description=${spec.description || ""}
      placeholder=${spec.placeholder || ""}
    ></gw-textarea>`;
  if (spec.type === "password")
    return html`<gw-password-input
      name=${spec.name}
      label=${spec.label}
      .value=${String(value ?? "")}
      ?required=${spec.required}
      description=${spec.description || ""}
      autocomplete=${spec.autocomplete || "new-password"}
    ></gw-password-input>`;
  return html`<gw-input
    name=${spec.name}
    label=${spec.label}
    type=${spec.type || "text"}
    .value=${String(value ?? "")}
    ?required=${spec.required}
    ?readonly=${spec.readonly}
    description=${spec.description || ""}
    placeholder=${spec.placeholder || ""}
    min=${spec.min || ""}
    max=${spec.max || ""}
    step=${spec.step || ""}
    autocomplete=${spec.autocomplete || ""}
  ></gw-input>`;
}
export function readForm(event: Event): FormData {
  event.preventDefault();
  return new FormData(event.currentTarget as HTMLFormElement);
}
export const text = (data: FormData, name: string) => String(data.get(name) ?? "").trim();
export const secret = (data: FormData, name: string) => String(data.get(name) ?? "");
export function formValues(data: FormData, fields: FieldSpec[]): Record<string, unknown> {
  return Object.fromEntries(
    fields.map((spec) => {
      const value = text(data, spec.name);
      return [
        spec.name,
        spec.type === "boolean"
          ? data.has(spec.name)
          : spec.type === "number"
            ? value === ""
              ? null
              : Number(value)
            : spec.type === "json"
              ? JSON.parse(value || "{}")
              : spec.type === "password"
                ? secret(data, spec.name)
                : value,
      ];
    }),
  );
}
export function heading(title: string, description = "", action: unknown = nothing) {
  return html`<header class="page-heading">
    <div>
      <h1>${title}</h1>
      ${description ? html`<p>${description}</p>` : nothing}
    </div>
    <div class="actions">${action}</div>
  </header>`;
}
export function badge(value: string | boolean | null | undefined) {
  const label = value === true ? "Enabled" : value === false ? "Disabled" : value || "Unknown";
  const tone =
    /^(active|ready|success|approved|verified|enabled|complete|online|applied|resolved|running)$/i.test(
      label,
    )
      ? "success"
      : /failed|error|rejected|disabled|revoked|offline/i.test(label)
        ? "error"
        : /pending|building|syncing|queued|expiring|backoff|draining|unresolved|claimed/i.test(
              label,
            )
          ? "warning"
          : "neutral";
  return html`<gw-badge tone=${tone}>${label.replaceAll("_", " ")}</gw-badge>`;
}
export function table<T>(
  rows: T[],
  columns: { label: string; key?: keyof T; render?: (row: T) => unknown }[],
  caption = "Results",
) {
  return html`<gw-table
    .rows=${rows}
    .columns=${columns.map((column) => ({ ...column, render: column.render ? (row: unknown) => column.render!(row as T) : undefined }))}
    caption=${caption}
  ></gw-table>`;
}
export function details(value: Record<string, unknown>) {
  return html`<dl class="details">
    ${Object.entries(value).map(
      ([key, value]) =>
        html`<div>
          <dt>${key.replaceAll("_", " ")}</dt>
          <dd>
            ${typeof value === "object" && value !== null ? html`<pre>${JSON.stringify(value, null, 2)}</pre>` : String(value ?? "—")}
          </dd>
        </div>`,
    )}
  </dl>`;
}
