import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { Button } from "./button";

describe("shadcn ui components", () => {
  test("button uses the standard shadcn class tokens", () => {
    render(<Button>Sign in</Button>);

    const button = screen.getByRole("button", { name: "Sign in" });

    expect(button.getAttribute("data-slot")).toBe("button");
    expect(button.className).toContain("group/button");
    expect(button.className).toContain("rounded-lg");
    expect(button.className).toContain("bg-primary");
  });
});
