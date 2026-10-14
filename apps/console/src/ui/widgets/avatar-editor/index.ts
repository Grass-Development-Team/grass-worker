import { errorMessage } from "@/infra/serializer";
import { GwElement, define, sharedStyles } from "@/ui/components/base";
import { notify } from "@/ui/components/toast";
import { css, html } from "lit";
import { avatarCropToPng, calculateAvatarCrop, clampAvatarOffset } from "./crop";
export class AvatarEditor extends GwElement {
  static properties = {
    src: {},
    label: {},
    imageUrl: { state: true },
    zoom: { state: true },
    x: { state: true },
    y: { state: true },
    busy: { type: Boolean },
  };
  static styles = [
    sharedStyles,
    css`
      .editor {
        display: grid;
        gap: 16px;
        justify-items: start;
      }
      .crop {
        width: 240px;
        height: 240px;
        overflow: hidden;
        border-radius: 50%;
        background: var(--control);
        touch-action: none;
        position: relative;
      }
      .crop img {
        position: absolute;
        max-width: none;
        left: 50%;
        top: 50%;
        user-select: none;
        pointer-events: none;
      }
      .actions {
        display: flex;
        gap: 8px;
      }
      input[type="file"] {
        max-width: 100%;
        font-size: 12px;
      }
      input[type="range"] {
        width: 240px;
        accent-color: var(--primary);
      }
      p {
        color: var(--muted);
        font-size: 12px;
      }
    `,
  ];
  src = "";
  label = "Avatar";
  imageUrl = "";
  zoom = 1;
  x = 0;
  y = 0;
  busy = false;
  private image: HTMLImageElement | null = null;
  private drag: { x: number; y: number; ox: number; oy: number } | null = null;
  private revision = 0;
  private crop() {
    return {
      imageWidth: this.image?.naturalWidth || 1,
      imageHeight: this.image?.naturalHeight || 1,
      viewportSize: 240,
      zoom: this.zoom,
      offsetX: this.x,
      offsetY: this.y,
    };
  }
  disconnectedCallback() {
    this.revision++;
    if (this.imageUrl) URL.revokeObjectURL(this.imageUrl);
    super.disconnectedCallback();
  }
  private async choose(event: Event) {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (!file) return;
    const version = ++this.revision;
    try {
      if (file.size > 10 * 1024 * 1024) throw new Error("Choose an image smaller than 10 MB.");
      if (!["image/png", "image/jpeg", "image/webp"].includes(file.type))
        throw new Error("Choose a PNG, JPEG, or WebP image.");
      const url = URL.createObjectURL(file),
        image = new Image();
      image.src = url;
      try {
        await image.decode();
      } catch {
        URL.revokeObjectURL(url);
        throw new Error("Could not read this image.");
      }
      if (version !== this.revision) {
        URL.revokeObjectURL(url);
        return;
      }
      if (this.imageUrl) URL.revokeObjectURL(this.imageUrl);
      this.image = image;
      this.imageUrl = url;
      this.zoom = 1;
      this.x = 0;
      this.y = 0;
    } catch (cause) {
      notify(errorMessage(cause), "error");
    }
  }
  private clamp() {
    const offset = clampAvatarOffset(this.crop());
    this.x = offset.x;
    this.y = offset.y;
  }
  private async save() {
    if (!this.image) return;
    try {
      const png = await avatarCropToPng(this.image, calculateAvatarCrop(this.crop()));
      this.dispatchEvent(
        new CustomEvent("gw-avatar", { detail: { png }, bubbles: true, composed: true }),
      );
    } catch (cause) {
      notify(errorMessage(cause), "error");
    }
  }
  render() {
    const scale = this.image
      ? Math.max(240 / this.image.naturalWidth, 240 / this.image.naturalHeight) * this.zoom
      : 1;
    return html`<div class="editor">
      <gw-avatar src=${this.src} alt=${this.label} style="width:64px;height:64px"></gw-avatar
      ><input
        type="file"
        accept="image/png,image/jpeg,image/webp"
        aria-label="Choose avatar image"
        ?disabled=${this.busy}
        @change=${this.choose}
      />${
        this.imageUrl
          ? html`<div
                class="crop"
                role="img"
                aria-label="Avatar crop preview. Drag to reposition."
                @pointerdown=${(event: PointerEvent) => {
                  this.drag = { x: event.clientX, y: event.clientY, ox: this.x, oy: this.y };
                  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
                }}
                @pointermove=${(event: PointerEvent) => {
                  if (this.drag) {
                    this.x = this.drag.ox + event.clientX - this.drag.x;
                    this.y = this.drag.oy + event.clientY - this.drag.y;
                    this.clamp();
                  }
                }}
                @pointerup=${() => (this.drag = null)}
                @pointercancel=${() => (this.drag = null)}
              >
                <img
                  src=${this.imageUrl}
                  alt=""
                  style=${`width:${this.image!.naturalWidth * scale}px;height:${this.image!.naturalHeight * scale}px;transform:translate(calc(-50% + ${this.x}px),calc(-50% + ${this.y}px))`}
                />
              </div>
              <label
                >Zoom
                <input
                  type="range"
                  min="1"
                  max="4"
                  step="0.05"
                  .value=${String(this.zoom)}
                  @input=${(event: Event) => {
                    this.zoom = Number((event.target as HTMLInputElement).value);
                    this.clamp();
                  }}
              /></label>
              <div class="actions">
                <gw-button .loading=${this.busy} @click=${this.save}>Save avatar</gw-button
                ><gw-button
                  variant="outline"
                  @click=${() => {
                    URL.revokeObjectURL(this.imageUrl);
                    this.imageUrl = "";
                    this.image = null;
                  }}
                  >Cancel</gw-button
                >
              </div>`
          : this.src
            ? html`<gw-button
                variant="outline"
                ?disabled=${this.busy}
                @click=${() => this.dispatchEvent(new CustomEvent("gw-avatar-remove", { bubbles: true, composed: true }))}
                >Remove avatar</gw-button
              >`
            : ""
      }
      <p>PNG, JPEG, or WebP. Drag the preview and adjust zoom to crop.</p>
    </div>`;
  }
}
define("gw-avatar-editor", AvatarEditor);
