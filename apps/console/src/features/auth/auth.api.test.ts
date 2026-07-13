import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { getCsrfToken, setCsrfToken } from "@/lib/csrf";
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
        user: { id: "user-1", email: "leo@example.com", display_name: "Leo" },
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
});
