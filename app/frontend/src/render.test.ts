import { describe, expect, test } from "bun:test";
import { renderHomePage } from "./render";

describe("renderHomePage", () => {
  test("renders hello world placeholder and service urls", () => {
    const html = renderHomePage({
      apiBaseUrl: "http://127.0.0.1:3000",
      nodeBaseUrl: "http://127.0.0.1:3001",
    });

    expect(html).toContain("Hello, World");
    expect(html).toContain("http://127.0.0.1:3000");
    expect(html).toContain("http://127.0.0.1:3001");
  });
});
