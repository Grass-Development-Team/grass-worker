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

  it("registers with a local return destination and stores the returned csrf token", async () => {
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
      return_to: "/invitations/accept?token=invite-token",
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
          return_to: "/invitations/accept?token=invite-token",
        }),
      }),
    );
    expect(getCsrfToken()).toBe("csrf-token");
  });

  it("keeps csrf empty while registration is waiting for email verification", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({ verification_required: true, email: "leo@example.com" }),
    );

    const result = await authApi.register({
      email: "leo@example.com",
      display_name: "Leo",
      password: "correct horse battery staple",
    });

    expect(result).toEqual({ verification_required: true, email: "leo@example.com" });
    expect(getCsrfToken()).toBeNull();
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

  it("updates the current display name", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({
        user: {
          id: "user-1",
          email: "leo@example.com",
          display_name: "Leonard",
          platform_role: "user",
        },
      }),
    );

    await authApi.updateMe({ display_name: "Leonard" });

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/me",
      expect.objectContaining({
        method: "PATCH",
        body: JSON.stringify({ display_name: "Leonard" }),
      }),
    );
  });
});
