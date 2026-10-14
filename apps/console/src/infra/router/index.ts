import { Store } from "@/infra/state";
export interface LocationState {
  pathname: string;
  search: URLSearchParams;
  hash: string;
}
function current(): LocationState {
  return {
    pathname: window.location.pathname,
    search: new URLSearchParams(window.location.search),
    hash: window.location.hash,
  };
}
export const location = new Store<LocationState>(current());
export function navigate(path: string, replace = false): void {
  const target = new URL(path, window.location.origin);
  if (target.origin !== window.location.origin) {
    window.location.assign(target.href);
    return;
  }
  history[replace ? "replaceState" : "pushState"](
    null,
    "",
    `${target.pathname}${target.search}${target.hash}`,
  );
  location.set(current());
}
export function matchPath(pattern: string, pathname: string): Record<string, string> | null {
  const parts = pattern.split("/").filter(Boolean),
    actual = pathname.split("/").filter(Boolean);
  const params: Record<string, string> = {};
  if (parts.length !== actual.length) return null;
  for (let index = 0; index < parts.length; index++) {
    if (parts[index].startsWith(":")) {
      try {
        const value = decodeURIComponent(actual[index]);
        if (
          !value ||
          value === "." ||
          value === ".." ||
          value.includes("/") ||
          value.includes(String.fromCharCode(92)) ||
          [...value].some((character) => character.charCodeAt(0) < 32)
        )
          return null;
        params[parts[index].slice(1)] = value;
      } catch {
        return null;
      }
    } else if (parts[index] !== actual[index]) return null;
  }
  return params;
}
export function listenNavigation(): () => void {
  const pop = () => location.set(current());
  const click = (event: MouseEvent) => {
    if (
      event.defaultPrevented ||
      event.button ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey
    )
      return;
    const anchor = event.composedPath().find((item) => item instanceof HTMLAnchorElement) as
      HTMLAnchorElement | undefined;
    if (!anchor || anchor.target || anchor.hasAttribute("download")) return;
    const target = new URL(anchor.href, window.location.origin);
    if (
      target.origin !== window.location.origin ||
      target.pathname.startsWith("/api/") ||
      target.pathname.startsWith("/health")
    )
      return;
    event.preventDefault();
    navigate(`${target.pathname}${target.search}${target.hash}`);
  };
  window.addEventListener("popstate", pop);
  window.addEventListener("hashchange", pop);
  document.addEventListener("click", click);
  return () => {
    window.removeEventListener("popstate", pop);
    window.removeEventListener("hashchange", pop);
    document.removeEventListener("click", click);
  };
}
