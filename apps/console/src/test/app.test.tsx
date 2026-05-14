import { describe, expect, it } from "vitest";

import { App } from "../app";

describe("App", () => {
  it("exports the console app component", () => {
    expect(App).toBeTypeOf("function");
  });
});
