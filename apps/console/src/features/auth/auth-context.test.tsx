import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { API_UNAUTHORIZED_EVENT } from "@/lib/api";
import { getCsrfToken, setCsrfToken } from "@/lib/csrf";
import { authApi } from "./auth.api";
import { AuthProvider, useAuth } from "./auth-context";

vi.mock("./auth.api", () => ({
  authApi: {
    restore: vi.fn(),
    login: vi.fn(),
    register: vi.fn(),
    updateMe: vi.fn(),
    logout: vi.fn(),
  },
}));

afterEach(() => {
  vi.clearAllMocks();
  setCsrfToken(null);
});

it("updates the current user after saving the profile", async () => {
  vi.mocked(authApi.restore).mockResolvedValue({
    user: {
      id: "user-1",
      email: "leo@example.com",
      display_name: "Leo",
      platform_role: "user",
    },
  });
  vi.mocked(authApi.updateMe).mockResolvedValue({
    user: {
      id: "user-1",
      email: "leo@example.com",
      display_name: "Leonard",
      platform_role: "user",
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <AuthProvider>{children}</AuthProvider>
  );
  const { result } = renderHook(() => useAuth(), { wrapper });
  await waitFor(() => expect(result.current.user?.display_name).toBe("Leo"));

  await act(() => result.current.updateProfile("Leonard"));

  expect(authApi.updateMe).toHaveBeenCalledWith({ display_name: "Leonard" });
  expect(result.current.user?.display_name).toBe("Leonard");
});

it("clears local authentication when an API request reports an expired session", async () => {
  vi.mocked(authApi.restore).mockResolvedValue({
    user: {
      id: "user-1",
      email: "leo@example.com",
      display_name: "Leo",
      platform_role: "user",
    },
  });
  setCsrfToken("csrf-token");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <AuthProvider>{children}</AuthProvider>
  );
  const { result } = renderHook(() => useAuth(), { wrapper });
  await waitFor(() => expect(result.current.user?.id).toBe("user-1"));

  act(() => window.dispatchEvent(new Event(API_UNAUTHORIZED_EVENT)));

  expect(result.current.user).toBeNull();
  expect(getCsrfToken()).toBeNull();
});
