export function safeLocalReturnTo(
  value: string | null | undefined,
  origin = window.location.origin,
): string | null {
  if (
    !value ||
    value.length > 4096 ||
    !value.startsWith("/") ||
    value.startsWith("//") ||
    value.includes("\\") ||
    [...value].some((character) => {
      const code = character.charCodeAt(0);
      return code <= 0x1f || code === 0x7f;
    })
  ) {
    return null;
  }

  const destination = new URL(value, origin);
  if (destination.origin !== origin) return null;
  return `${destination.pathname}${destination.search}${destination.hash}`;
}

export function authHref(path: "/login" | "/signup", returnTo: string | null): string {
  return returnTo ? `${path}?${new URLSearchParams({ return_to: returnTo })}` : path;
}

export function previewAuthorizationContinuation(
  search: string,
  origin = window.location.origin,
): string | null {
  const value = new URLSearchParams(search).get("continue");
  if (
    !value ||
    value.length > 4096 ||
    !value.startsWith("/api/v1/preview/authorize?") ||
    value.startsWith("//") ||
    value.includes("\\") ||
    [...value].some((character) => {
      const code = character.charCodeAt(0);
      return code <= 0x1f || code === 0x7f;
    })
  ) {
    return null;
  }

  const destination = new URL(value, origin);
  if (
    destination.origin !== origin ||
    destination.pathname !== "/api/v1/preview/authorize" ||
    destination.hash ||
    destination.searchParams.getAll("state").length !== 1 ||
    !destination.searchParams.get("state")
  ) {
    return null;
  }
  return `${destination.pathname}${destination.search}`;
}
