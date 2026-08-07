import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { getErrorMessage, showErrorToast } from "./toast";
import { queryClient } from "./query-client";

const { errorToast } = vi.hoisted(() => ({ errorToast: vi.fn() }));

vi.mock("sonner", () => ({
  toast: {
    error: errorToast,
  },
}));

afterEach(() => {
  queryClient.clear();
  vi.clearAllMocks();
});

describe("getErrorMessage", () => {
  it("uses Error messages", () => {
    expect(getErrorMessage(new Error("Request failed"))).toBe("Request failed");
  });

  it("uses the fallback for empty Error messages", () => {
    for (const message of ["", "  \n  "]) {
      expect(getErrorMessage(new Error(message))).toBe("Something went wrong.");
    }
  });

  it("uses messages from API-like errors", () => {
    expect(getErrorMessage({ message: "Registration is closed" })).toBe("Registration is closed");
  });

  it("uses the fallback for empty API-like messages", () => {
    for (const message of ["", "\t"]) {
      expect(getErrorMessage({ message })).toBe("Something went wrong.");
    }
  });

  it("uses a stable message for unknown values", () => {
    expect(getErrorMessage("sensitive thrown value")).toBe("Something went wrong.");
  });

  it("shows the formatted error in an error toast", () => {
    showErrorToast({ message: "Unable to register" });

    expect(errorToast).toHaveBeenCalledWith("Unable to register");
  });
});

describe("queryClient", () => {
  it("preserves the shared query defaults", () => {
    expect(queryClient.getDefaultOptions().queries).toMatchObject({
      staleTime: 5_000,
      retry: 1,
    });
  });

  it("shows query errors in a toast", async () => {
    await expect(
      queryClient.fetchQuery({
        queryKey: ["failed-query"],
        queryFn: () => Promise.reject(new Error("Unable to load projects")),
        retry: false,
      }),
    ).rejects.toThrow("Unable to load projects");

    expect(errorToast).toHaveBeenCalledWith("Unable to load projects", {
      id: 'query-error:["failed-query"]',
    });
  });

  it("uses a stable toast ID for repeated failures of the same query", async () => {
    const queryKey = ["polling-status"];
    const query = {
      queryKey,
      queryFn: () => Promise.reject(new Error("Service unavailable")),
      retry: false,
    };

    await expect(queryClient.fetchQuery(query)).rejects.toThrow("Service unavailable");
    await expect(queryClient.fetchQuery(query)).rejects.toThrow("Service unavailable");

    const queryHash = queryClient.getQueryCache().find({ queryKey })?.queryHash;
    expect(queryHash).toBeDefined();
    expect(errorToast).toHaveBeenNthCalledWith(1, "Service unavailable", {
      id: `query-error:${queryHash}`,
    });
    expect(errorToast).toHaveBeenNthCalledWith(2, "Service unavailable", {
      id: `query-error:${queryHash}`,
    });
  });

  it("shows mutation errors in a toast", async () => {
    const mutation = queryClient.getMutationCache().build(queryClient, {
      mutationFn: () => Promise.reject(new Error("Unable to create project")),
    });

    await expect(mutation.execute(undefined)).rejects.toThrow("Unable to create project");

    expect(errorToast).toHaveBeenCalledWith("Unable to create project");
  });
});
