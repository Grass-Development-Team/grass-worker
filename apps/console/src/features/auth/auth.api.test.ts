import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { getCsrfToken, setCsrfToken } from "@/lib/csrf";
import { request } from "@/lib/api";
import { authApi } from "./auth.api";

const response = (data: unknown) =>
  new Response(JSON.stringify({ code: 200, message: "ok", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });

describe("authApi.register", () => {
  afterEach(() => {
    setCsrfToken(null);
    vi.restoreAllMocks();
  });

  it("registers with an invitation token and stores the returned csrf token", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({
        user: {
          id: "user-1",
          email: "leo@example.com",
          display_name: "Leo",
          platform_role: "user",
        },
        csrf_token: "csrf-token",
      }),
    );

    await authApi.register({
      email: "leo@example.com",
      display_name: "Leo",
      password: "correct horse battery staple",
      invitation_token: "invite-token",
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/auth/register",
      expect.objectContaining({
        method: "POST",
        credentials: "include",
        body: JSON.stringify({
          email: "leo@example.com",
          display_name: "Leo",
          password: "correct horse battery staple",
          invitation_token: "invite-token",
        }),
      }),
    );
    expect(getCsrfToken()).toBe("csrf-token");
  });

  it("restores the csrf token before a mutation is sent", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        response({
          user: {
            id: "user-1",
            email: "leo@example.com",
            display_name: "Leo",
            platform_role: "user",
          },
        }),
      )
      .mockResolvedValueOnce(response({ csrf_token: "restored-token" }))
      .mockResolvedValueOnce(response({ ok: true }));

    await authApi.restore();
    await request<{ ok: boolean }>("/api/v1/teams", {
      method: "POST",
      body: JSON.stringify({ name: "Team", slug: "team" }),
    });

    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "/api/v1/auth/csrf",
      expect.objectContaining({ credentials: "include" }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      3,
      "/api/v1/teams",
      expect.objectContaining({
        headers: expect.objectContaining({ "x-csrf-token": "restored-token" }),
      }),
    );
  });

  it("shares concurrent session restoration to avoid replacing the csrf token", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        response({
          user: {
            id: "user-1",
            email: "leo@example.com",
            display_name: "Leo",
            platform_role: "user",
          },
        }),
      )
      .mockResolvedValueOnce(response({ csrf_token: "restored-token" }))
      .mockResolvedValueOnce(
        response({
          user: {
            id: "user-1",
            email: "leo@example.com",
            display_name: "Leo",
            platform_role: "user",
          },
        }),
      )
      .mockResolvedValueOnce(response({ csrf_token: "stale-token" }));

    const [first, second] = await Promise.all([authApi.restore(), authApi.restore()]);

    expect(first).toEqual(second);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(getCsrfToken()).toBe("restored-token");
  });
});
