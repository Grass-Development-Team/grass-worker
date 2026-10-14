export function flattenConfiguration(
  value: Record<string, unknown>,
  prefix = "",
): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [key, item] of Object.entries(value)) {
    const name = prefix ? `${prefix}.${key}` : key;
    if (item !== null && typeof item === "object" && !Array.isArray(item))
      Object.assign(result, flattenConfiguration(item as Record<string, unknown>, name));
    else result[name] = item;
  }
  return result;
}
export function expandConfiguration(value: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [path, item] of Object.entries(value)) {
    const segments = path.split(".");
    if (segments.some((segment) => ["__proto__", "prototype", "constructor"].includes(segment)))
      throw new Error("Invalid configuration field.");
    let current = result;
    for (const key of segments.slice(0, -1)) {
      current[key] ??= {};
      current = current[key] as Record<string, unknown>;
    }
    current[segments.at(-1)!] = item;
  }
  return result;
}
