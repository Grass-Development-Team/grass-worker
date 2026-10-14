import { describe, expect, it } from "vitest";
import { expandConfiguration, flattenConfiguration } from "./configuration";
describe("node configuration forms", () => {
  it("roundtrips nested values and structured target arrays", () => {
    const config = {
      node: { id: "one", capabilities: { build: false, serve: true } },
      serve: { port: 7818 },
      security: {
        private_repository_targets: [{ host: "git.example.com", ip: "192.0.2.1", port: 22 }],
      },
    };
    expect(expandConfiguration(flattenConfiguration(config))).toEqual(config);
  });
  it.each(["__proto__.polluted", "node.constructor.value", "node.prototype.value"])(
    "rejects prototype field %s",
    (key) => {
      expect(() => expandConfiguration({ [key]: true })).toThrow("Invalid configuration field");
      expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    },
  );
});
