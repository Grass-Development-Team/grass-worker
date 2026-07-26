import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { API_UNAUTHORIZED_EVENT, ApiError, request } from "./api";

describe("request", () => {
  afterEach(() => vi.restoreAllMocks());

  it("reports a useful error when an upstream returns a non-json response", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("Bad gateway", {
        status: 502,
        headers: { "Content-Type": "text/plain" },
      }),
    );

    await expect(request("/api/v1/teams")).rejects.toMatchObject({
      name: "ApiError",
      message: "Request failed (502)",
      status: 502,
    });
  });

  it("preserves structured API error details", async () => {
    const unauthorized = vi.fn();
    window.addEventListener(API_UNAUTHORIZED_EVENT, unauthorized, { once: true });
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          code: 40101,
          message: "authentication required",
          data: null,
          op: "session.required",
        }),
        { status: 401, headers: { "Content-Type": "application/json" } },
      ),
    );

    const error = await request("/api/v1/teams/team-1").catch((cause) => cause);

    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({
      message: "authentication required",
      status: 401,
      code: 40101,
      operation: "session.required",
    });
    expect(unauthorized).toHaveBeenCalledOnce();
  });

  it("does not force a json content type onto bodyless requests", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ code: 200, message: "OK", data: { teams: [] } }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );

    await request("/api/v1/teams");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/teams",
      expect.objectContaining({ headers: {} }),
    );
  });
});
