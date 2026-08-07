import { describe, expect, it } from "vite-plus/test";

import { calculateAvatarCrop, clampAvatarOffset } from "./avatar-crop";

describe("avatar crop math", () => {
  it("uses the largest source-pixel square supported by the crop", () => {
    expect(
      calculateAvatarCrop({
        imageWidth: 800,
        imageHeight: 600,
        viewportSize: 320,
        zoom: 1,
        offsetX: 0,
        offsetY: 0,
      }),
    ).toEqual({ sourceX: 100, sourceY: 0, sourceSize: 600, outputSize: 600 });

    expect(
      calculateAvatarCrop({
        imageWidth: 800,
        imageHeight: 600,
        viewportSize: 320,
        zoom: 2,
        offsetX: 0,
        offsetY: 0,
      }),
    ).toEqual({ sourceX: 250, sourceY: 150, sourceSize: 300, outputSize: 300 });
  });

  it("clamps output dimensions between 128 and 1024", () => {
    expect(
      calculateAvatarCrop({
        imageWidth: 1,
        imageHeight: 1,
        viewportSize: 320,
        zoom: 1,
        offsetX: 0,
        offsetY: 0,
      }).outputSize,
    ).toBe(128);
    expect(
      calculateAvatarCrop({
        imageWidth: 2400,
        imageHeight: 1600,
        viewportSize: 320,
        zoom: 1,
        offsetX: 0,
        offsetY: 0,
      }).outputSize,
    ).toBe(1024);
  });

  it("keeps the image covering the viewport while dragging", () => {
    expect(
      clampAvatarOffset({
        imageWidth: 800,
        imageHeight: 600,
        viewportSize: 320,
        zoom: 1,
        offsetX: 999,
        offsetY: -999,
      }),
    ).toEqual({ x: 53.33333333333334, y: 0 });
  });
});
