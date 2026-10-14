import { afterEach, describe, expect, it, vi } from "vitest";
import { changeTheme, springProgress, THEME_DURATION } from ".";

const root = { dataset: { theme: "light" }, classList: { add: vi.fn(), remove: vi.fn() } };
afterEach(() => {
  vi.restoreAllMocks();
  root.dataset.theme = "light";
});
describe("theme transitions", () => {
  it("has settled endpoints and a damped spring response", () => {
    expect(THEME_DURATION).toBe(300);
    expect(springProgress(0)).toBe(0);
    expect(springProgress(1)).toBe(1);
    expect(springProgress(0.5)).toBeGreaterThan(0.5);
    expect(
      Math.max(...Array.from({ length: 61 }, (_, index) => springProgress(index / 60))),
    ).toBeGreaterThan(1);
    expect(springProgress(0.95)).toBeCloseTo(1, 2);
  });
  it("updates and saves the theme when View Transition is unavailable", async () => {
    vi.stubGlobal("document", { documentElement: root });
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    await changeTheme("dark");
    expect(root.dataset.theme).toBe("dark");
    expect(localStorage.getItem("grass-worker.theme")).toBe("dark");
  });
  it("avoids taking snapshots for reduced motion", async () => {
    const startViewTransition = vi.fn();
    vi.stubGlobal("document", { documentElement: root, startViewTransition });
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduced-motion") }));
    await changeTheme("dark");
    expect(root.dataset.theme).toBe("dark");
    expect(startViewTransition).not.toHaveBeenCalled();
  });
});
