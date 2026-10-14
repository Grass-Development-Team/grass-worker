import {
  storageDefaults,
  storageInput,
  type StorageBackend,
  type StorageConfiguration,
  type StorageInput,
} from "@/services/storage/form";
import { define } from "@/ui/components/base";
import { field, secret, text } from "@/ui/pages/base";
import { html, LitElement, type PropertyValues } from "lit";
export class StorageForm extends LitElement {
  static properties = { configuration: { attribute: false }, backend: { state: true } };
  configuration: StorageConfiguration | null = null;
  backend: StorageBackend = "local";
  protected createRenderRoot() {
    return this;
  }
  protected willUpdate(changed: PropertyValues): void {
    if (changed.has("configuration") && this.configuration)
      this.backend = this.configuration.backend;
  }
  input(data: FormData): StorageInput {
    return storageInput({
      backend: this.backend,
      localRoot: text(data, "local_root"),
      endpoint: text(data, "endpoint"),
      region: text(data, "region"),
      bucket: text(data, "bucket"),
      prefix: text(data, "prefix"),
      forcePathStyle: data.has("force_path_style"),
      allowHttp: data.has("allow_http"),
      accessKeyId: secret(data, "access_key_id"),
      secretAccessKey: secret(data, "secret_access_key"),
      sessionToken: secret(data, "session_token"),
    });
  }
  render() {
    const value = this.configuration;
    const defaults = storageDefaults(this.backend);
    return html`<div class="stack">
      <gw-select
        name="backend"
        label="Storage backend"
        .value=${this.backend}
        .options=${["local", "s3", "minio", "r2"].map((value) => ({ value, label: value === "local" ? "Local filesystem" : value.toUpperCase() }))}
        @gw-change=${(event: CustomEvent<{ value: StorageBackend }>) => (this.backend = event.detail.value)}
      ></gw-select
      >${field({ name: "local_root", label: "Local storage path", required: true }, value?.local_root || "/data")}${
        this.backend !== "local"
          ? html`<div class="grid">
                ${field({ name: "endpoint", label: "Endpoint URL", type: "url" }, value?.endpoint)}${field({ name: "region", label: "Region", required: true }, value?.region || defaults.region)}${field({ name: "bucket", label: "Bucket", required: true }, value?.bucket)}${field({ name: "prefix", label: "Key prefix" }, value?.prefix)}
              </div>
              ${field({ name: "force_path_style", label: "Use path-style URLs", type: "boolean" }, value?.force_path_style ?? defaults.forcePathStyle)}${field({ name: "allow_http", label: "Allow HTTP for local object storage", type: "boolean" }, value?.allow_http ?? defaults.allowHttp)}
              <div class="grid">
                ${field({ name: "access_key_id", label: "Access key ID", type: "password", description: value?.credentials_configured ? "Leave blank to preserve configured credentials." : "" })}${field({ name: "secret_access_key", label: "Secret access key", type: "password" })}${field({ name: "session_token", label: "Session token (optional)", type: "password" })}
              </div>`
          : ""
      }
    </div>`;
  }
}
define("gw-storage-form", StorageForm);
