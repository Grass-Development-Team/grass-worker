import { describe, expect, it } from "vitest";

import { App } from "../App";

describe("App", () => {
  it("exports the console app component", () => {
    expect(App).toBeTypeOf("function");
  });
});
