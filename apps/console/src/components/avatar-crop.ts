export interface AvatarCropInput {
  imageWidth: number;
  imageHeight: number;
  viewportSize: number;
  zoom: number;
  offsetX: number;
  offsetY: number;
}

export interface AvatarCrop {
  sourceX: number;
  sourceY: number;
  sourceSize: number;
  outputSize: number;
}

function renderedScale(input: AvatarCropInput): number {
  const baseScale = Math.max(
    input.viewportSize / input.imageWidth,
    input.viewportSize / input.imageHeight,
  );
  return baseScale * Math.max(1, input.zoom);
}

export function clampAvatarOffset(input: AvatarCropInput): { x: number; y: number } {
  const scale = renderedScale(input);
  const maxX = Math.max(0, (input.imageWidth * scale - input.viewportSize) / 2);
  const maxY = Math.max(0, (input.imageHeight * scale - input.viewportSize) / 2);
  return {
    x: maxX === 0 ? 0 : Math.min(maxX, Math.max(-maxX, input.offsetX)),
    y: maxY === 0 ? 0 : Math.min(maxY, Math.max(-maxY, input.offsetY)),
  };
}

export function calculateAvatarCrop(input: AvatarCropInput): AvatarCrop {
  const scale = renderedScale(input);
  const offset = clampAvatarOffset(input);
  const sourceSize = Math.min(input.imageWidth, input.imageHeight, input.viewportSize / scale);
  const sourceX = Math.min(
    input.imageWidth - sourceSize,
    Math.max(0, input.imageWidth / 2 - offset.x / scale - sourceSize / 2),
  );
  const sourceY = Math.min(
    input.imageHeight - sourceSize,
    Math.max(0, input.imageHeight / 2 - offset.y / scale - sourceSize / 2),
  );
  return {
    sourceX,
    sourceY,
    sourceSize,
    outputSize: Math.min(1024, Math.max(128, Math.round(sourceSize))),
  };
}

export function avatarCropToPng(image: CanvasImageSource, crop: AvatarCrop): Promise<Blob> {
  const canvas = document.createElement("canvas");
  canvas.width = crop.outputSize;
  canvas.height = crop.outputSize;
  const context = canvas.getContext("2d");
  if (!context) return Promise.reject(new Error("Canvas is unavailable in this browser."));
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  context.drawImage(
    image,
    crop.sourceX,
    crop.sourceY,
    crop.sourceSize,
    crop.sourceSize,
    0,
    0,
    crop.outputSize,
    crop.outputSize,
  );
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => (blob ? resolve(blob) : reject(new Error("Could not encode the cropped image."))),
      "image/png",
    );
  });
}
