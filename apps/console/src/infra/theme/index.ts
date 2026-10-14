import { Store } from "@/infra/state";
export type ThemePreference = "light" | "dark" | "system";
export const THEME_DURATION = 300;
export const theme = new Store<ThemePreference>(readPreference());
let changing = false;
function readPreference(): ThemePreference {
  try {
    const value = localStorage.getItem("grass-worker.theme");
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}
export function springProgress(position: number): number {
  if (position <= 0) return 0;
  if (position >= 1) return 1;
  const damping = 8,
    frequency = 12;
  const sample = (time: number) =>
    1 -
    Math.exp(-damping * time) *
      (Math.cos(frequency * time) + (damping / frequency) * Math.sin(frequency * time));
  return Math.max(0, sample(position) / sample(1));
}
export async function changeTheme(
  preference: ThemePreference,
  origin?: { x: number; y: number },
): Promise<void> {
  if (changing) return;
  const next =
    preference === "system"
      ? matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : preference;
  const apply = () => {
    document.documentElement.dataset.theme = next;
    theme.set(preference);
    try {
      localStorage.setItem("grass-worker.theme", preference);
    } catch {
      /* Storage may be unavailable. */
    }
  };
  if (
    document.documentElement.dataset.theme === next ||
    !document.startViewTransition ||
    matchMedia("(prefers-reduced-motion: reduce)").matches
  ) {
    apply();
    return;
  }
  changing = true;
  const x = origin?.x ?? innerWidth / 2,
    y = origin?.y ?? innerHeight / 2;
  const radius = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
  document.documentElement.classList.add("theme-transition");
  const transition = document.startViewTransition(apply);
  try {
    await transition.ready;
    const frames = Array.from({ length: 61 }, (_, index) => ({
      offset: index / 60,
      clipPath: `circle(${springProgress(index / 60) * radius}px at ${x}px ${y}px)`,
    }));
    await document.documentElement.animate(frames, {
      duration: THEME_DURATION,
      easing: "linear",
      fill: "both",
      pseudoElement: "::view-transition-new(root)",
    }).finished;
    await transition.finished;
  } catch {
    transition.skipTransition();
    apply();
  } finally {
    document.documentElement.classList.remove("theme-transition");
    changing = false;
  }
}
export function watchSystemTheme(): () => void {
  const media = matchMedia("(prefers-color-scheme: dark)");
  const listener = () => {
    if (theme.value === "system") void changeTheme("system");
  };
  media.addEventListener("change", listener);
  return () => media.removeEventListener("change", listener);
}
