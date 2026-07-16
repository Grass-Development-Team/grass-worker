import { describe, expect, it } from "vite-plus/test";

import { buildPostgresUrl } from "./setup.api";

describe("buildPostgresUrl", () => {
  it("encodes credentials and database names", () => {
    expect(
      buildPostgresUrl("db.example.com", "5432", "user name", "p@ss/word", "grass/worker"),
    ).toBe("postgres://user%20name:p%40ss%2Fword@db.example.com:5432/grass%2Fworker");
  });

  it("supports ipv6 database hosts", () => {
    expect(buildPostgresUrl("::1", "5432", "postgres", "password", "grass_worker")).toBe(
      "postgres://postgres:password@[::1]:5432/grass_worker",
    );
  });
});
