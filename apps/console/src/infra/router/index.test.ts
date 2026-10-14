import { previewAuthorizationContinuation, safeLocalReturnTo } from "@/services/auth/continuation";
import { describe, expect, it } from "vitest";
import { matchPath } from ".";

describe("navigation boundaries", () => {
  it("matches complete routes and decodes safe parameters", () => {
    expect(matchPath("/projects/:projectId", "/projects/site-1")).toEqual({ projectId: "site-1" });
    expect(matchPath("/projects/:projectId", "/projects/site-1/domains")).toBeNull();
    expect(matchPath("/", "/")).toEqual({});
  });
  it.each(["%2Fadmin", "%5Cadmin", "%00", "%2E%2E", "%", "%1F"])(
    "rejects unsafe path parameter %s",
    (value) => {
      expect(matchPath("/projects/:projectId", `/projects/${value}`)).toBeNull();
    },
  );
  it.each(["https://evil.example", "//evil.example", "/\\evil.example", "/\u0001bad"])(
    "rejects external or malformed return destinations",
    (value) => {
      expect(safeLocalReturnTo(value, "https://console.example.com")).toBeNull();
    },
  );
  it("preserves a local path, query, and fragment", () => {
    expect(safeLocalReturnTo("/projects?tab=one#details", "https://console.example.com")).toBe(
      "/projects?tab=one#details",
    );
  });
  it("allows only the expected preview authorization continuation", () => {
    const query = (value: string) => `?${new URLSearchParams({ continue: value })}`;
    expect(previewAuthorizationContinuation(query("/api/v1/preview/authorize?state=one"))).toBe(
      "/api/v1/preview/authorize?state=one",
    );
    for (const value of [
      "/api/v1/preview/authorize?state=one&state=two",
      "/api/v1/preview/authorize?state=",
      "/api/v1/preview/authorize?state=one#hash",
      "/api/v1/admin/users?state=one",
    ]) {
      expect(previewAuthorizationContinuation(query(value))).toBeNull();
    }
  });
});
