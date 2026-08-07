import { useEffect, useRef, useState } from "react";
import { CheckIcon, ImageUpIcon, Trash2Icon, ZoomInIcon } from "lucide-react";

import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Spinner } from "@/components/ui/spinner";
import { showErrorToast } from "@/lib/toast";

import {
  avatarCropToPng,
  calculateAvatarCrop,
  clampAvatarOffset,
  type AvatarCropInput,
} from "./avatar-crop";

interface AvatarEditorProps {
  src: string | null;
  fallback: string;
  onUpload: (png: Blob) => Promise<void>;
  onRemove: () => Promise<void>;
  disabled?: boolean;
  square?: boolean;
}

interface Dimensions {
  width: number;
  height: number;
}

interface Offset {
  x: number;
  y: number;
}

export function AvatarEditor({
  src,
  fallback,
  onUpload,
  onRemove,
  disabled = false,
  square = false,
}: AvatarEditorProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const imageRef = useRef<HTMLImageElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ pointerX: number; pointerY: number; offset: Offset } | null>(null);
  const [open, setOpen] = useState(false);
  const [objectUrl, setObjectUrl] = useState<string | null>(null);
  const [dimensions, setDimensions] = useState<Dimensions | null>(null);
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState<Offset>({ x: 0, y: 0 });
  const [pending, setPending] = useState(false);

  useEffect(
    () => () => {
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    },
    [objectUrl],
  );

  const cropInput = (nextOffset = offset, nextZoom = zoom): AvatarCropInput | null => {
    const viewportSize = viewportRef.current?.clientWidth ?? 0;
    if (!dimensions || viewportSize <= 0) return null;
    return {
      imageWidth: dimensions.width,
      imageHeight: dimensions.height,
      viewportSize,
      zoom: nextZoom,
      offsetX: nextOffset.x,
      offsetY: nextOffset.y,
    };
  };

  const setClampedOffset = (next: Offset, nextZoom = zoom) => {
    const input = cropInput(next, nextZoom);
    setOffset(input ? clampAvatarOffset(input) : next);
  };

  const chooseFile = (file: File | undefined) => {
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      showErrorToast(new Error("Choose an image file."));
      return;
    }
    if (objectUrl) URL.revokeObjectURL(objectUrl);
    setObjectUrl(URL.createObjectURL(file));
    setDimensions(null);
    setZoom(1);
    setOffset({ x: 0, y: 0 });
    setOpen(true);
    if (inputRef.current) inputRef.current.value = "";
  };

  const close = (nextOpen: boolean) => {
    if (pending) return;
    setOpen(nextOpen);
    if (!nextOpen) {
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      setObjectUrl(null);
      setDimensions(null);
    }
  };

  const save = async () => {
    const input = cropInput();
    const image = imageRef.current;
    if (!input || !image) return;
    setPending(true);
    try {
      const png = await avatarCropToPng(image, calculateAvatarCrop(input));
      await onUpload(png);
      setOpen(false);
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      setObjectUrl(null);
      setDimensions(null);
    } catch (cause) {
      showErrorToast(cause);
    } finally {
      setPending(false);
    }
  };

  const remove = async () => {
    setPending(true);
    try {
      await onRemove();
    } catch (cause) {
      showErrorToast(cause);
    } finally {
      setPending(false);
    }
  };

  const viewportSize = viewportRef.current?.clientWidth ?? 320;
  const baseScale = dimensions
    ? Math.max(viewportSize / dimensions.width, viewportSize / dimensions.height)
    : 1;
  const renderedWidth = dimensions ? dimensions.width * baseScale * zoom : viewportSize;
  const renderedHeight = dimensions ? dimensions.height * baseScale * zoom : viewportSize;

  return (
    <>
      <div className="flex flex-wrap items-center gap-4">
        <Avatar className={square ? "size-16 rounded-md" : "size-16"}>
          {src && <AvatarImage src={src} alt="" className="object-cover" />}
          <AvatarFallback className={square ? "rounded-md text-lg" : "text-lg"}>
            {fallback}
          </AvatarFallback>
        </Avatar>
        <div className="flex flex-wrap gap-2">
          <input
            ref={inputRef}
            type="file"
            accept="image/*"
            className="sr-only"
            onChange={(event) => chooseFile(event.target.files?.[0])}
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={disabled || pending}
            onClick={() => inputRef.current?.click()}
          >
            <ImageUpIcon /> {src ? "Replace" : "Upload"}
          </Button>
          {src && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={disabled || pending}
              onClick={remove}
            >
              {pending ? <Spinner /> : <Trash2Icon />} Remove
            </Button>
          )}
        </div>
      </div>

      <Dialog open={open} onOpenChange={close}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Crop avatar</DialogTitle>
            <DialogDescription className="sr-only">
              Position and scale the image inside the circular crop.
            </DialogDescription>
          </DialogHeader>
          <div className="mx-auto w-full max-w-80 space-y-5">
            <div
              ref={viewportRef}
              role="application"
              tabIndex={0}
              aria-label="Avatar crop area"
              className="relative aspect-square w-full touch-none overflow-hidden rounded-full border bg-muted outline-none ring-offset-background focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              onPointerDown={(event) => {
                event.currentTarget.setPointerCapture(event.pointerId);
                dragRef.current = {
                  pointerX: event.clientX,
                  pointerY: event.clientY,
                  offset,
                };
              }}
              onPointerMove={(event) => {
                if (!dragRef.current) return;
                setClampedOffset({
                  x: dragRef.current.offset.x + event.clientX - dragRef.current.pointerX,
                  y: dragRef.current.offset.y + event.clientY - dragRef.current.pointerY,
                });
              }}
              onPointerUp={(event) => {
                if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                  event.currentTarget.releasePointerCapture(event.pointerId);
                }
                dragRef.current = null;
              }}
              onPointerCancel={() => {
                dragRef.current = null;
              }}
              onKeyDown={(event) => {
                const movement: Record<string, Offset> = {
                  ArrowLeft: { x: -4, y: 0 },
                  ArrowRight: { x: 4, y: 0 },
                  ArrowUp: { x: 0, y: -4 },
                  ArrowDown: { x: 0, y: 4 },
                };
                const delta = movement[event.key];
                if (!delta) return;
                event.preventDefault();
                setClampedOffset({ x: offset.x + delta.x, y: offset.y + delta.y });
              }}
            >
              {objectUrl && (
                <img
                  ref={imageRef}
                  src={objectUrl}
                  alt=""
                  draggable={false}
                  className="pointer-events-none absolute left-1/2 top-1/2 max-w-none select-none"
                  style={{
                    width: renderedWidth,
                    height: renderedHeight,
                    transform: `translate(calc(-50% + ${offset.x}px), calc(-50% + ${offset.y}px))`,
                  }}
                  onLoad={(event) => {
                    setDimensions({
                      width: event.currentTarget.naturalWidth,
                      height: event.currentTarget.naturalHeight,
                    });
                    setOffset({ x: 0, y: 0 });
                  }}
                  onError={() => {
                    showErrorToast(new Error("This image could not be opened."));
                    close(false);
                  }}
                />
              )}
            </div>
            <div className="flex items-center gap-3">
              <ZoomInIcon className="size-4 shrink-0 text-muted-foreground" />
              <input
                type="range"
                min="1"
                max="4"
                step="0.01"
                value={zoom}
                aria-label="Zoom"
                className="h-5 w-full accent-foreground"
                onChange={(event) => {
                  const nextZoom = Number(event.target.value);
                  setZoom(nextZoom);
                  setClampedOffset(offset, nextZoom);
                }}
              />
            </div>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => close(false)} disabled={pending}>
              Cancel
            </Button>
            <Button type="button" onClick={save} disabled={!dimensions || pending}>
              {pending ? <Spinner /> : <CheckIcon />} Apply
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
