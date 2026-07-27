import { describe, expect, it } from "vite-plus/test";

import { credentialMatchesRepository } from "./project-settings-build-route";

describe("credentialMatchesRepository", () => {
  it("matches only scheme, normalized host, and effective port", () => {
    expect(
      credentialMatchesRepository(
        { kind: "https", host: "example.com", port: 443 },
        "https://EXAMPLE.com/org/repo.git",
      ),
    ).toBe(true);
    expect(
      credentialMatchesRepository(
        { kind: "https", host: "example.com", port: 443 },
        "https://example.com:8443/org/repo.git",
      ),
    ).toBe(false);
    expect(
      credentialMatchesRepository(
        { kind: "ssh", host: "example.com", port: 22 },
        "git@example.com:org/repo.git",
      ),
    ).toBe(true);
    expect(
      credentialMatchesRepository(
        { kind: "ssh", host: "example.com", port: 2222 },
        "ssh://git@example.com:2222/org/repo.git",
      ),
    ).toBe(true);
  });
});
