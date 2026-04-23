import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { Badge } from "./badge";
import { Button } from "./button";
import { Separator } from "./separator";
import { Skeleton } from "./skeleton";

describe("shadcn ui components", () => {
  test("button uses the standard shadcn class tokens", () => {
    render(<Button>Sign in</Button>);

    const button = screen.getByRole("button", { name: "Sign in" });

    expect(button.getAttribute("data-slot")).toBe("button");
    expect(button.className).toContain("group/button");
    expect(button.className).toContain("rounded-lg");
    expect(button.className).toContain("bg-primary");
  });

  test("badge renders shadcn badge classes", () => {
    render(<Badge>Active</Badge>);

    expect(screen.getByText("Active")).toHaveClass("inline-flex");
  });

  test("separator renders a decorative separator", () => {
    render(<Separator data-testid="separator" />);

    expect(screen.getByTestId("separator")).toHaveAttribute(
      "data-slot",
      "separator-root",
    );
  });

  test("skeleton renders loading placeholder classes", () => {
    render(<Skeleton data-testid="skeleton" />);

    expect(screen.getByTestId("skeleton")).toHaveClass("animate-pulse");
  });
});
