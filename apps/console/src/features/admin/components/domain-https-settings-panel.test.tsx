import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { DomainHttpsSettingsPanel } from "./domain-https-settings-panel";

function setup(configured = false) {
  let settings = {
    issuer: configured ? "zerossl" : "letsencrypt",
    zerossl_eab_configured: configured,
  };
  const patches: unknown[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (_input, init) => {
    if (init?.method === "PATCH") {
      const body = JSON.parse(String(init.body));
      patches.push(body);
      settings = {
        issuer: body.issuer,
        zerossl_eab_configured: settings.zerossl_eab_configured || !!body.eab_kid,
      };
    }
    return new Response(JSON.stringify({ code: 200, message: "OK", data: settings }), {
      headers: { "Content-Type": "application/json" },
    });
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <DomainHttpsSettingsPanel />
    </QueryClientProvider>,
  );
  return patches;
}
afterEach(() => vi.restoreAllMocks());
it("uses Let's Encrypt by default and derives contacts from the adding user", async () => {
  setup();
  expect(await screen.findByLabelText("Certificate authority")).toHaveTextContent("Let's Encrypt");
  expect(screen.getByText(/contact email comes from/)).toBeInTheDocument();
  expect(screen.queryByLabelText(/email|token/i)).not.toBeInTheDocument();
  expect(screen.getByText(/Automatic renewal is enabled by default/)).toBeInTheDocument();
});
it("saves ZeroSSL credentials and clears the write-only fields", async () => {
  const patches = setup();
  const user = userEvent.setup();
  await user.click(await screen.findByLabelText("Certificate authority"));
  await user.click(screen.getByRole("option", { name: "ZeroSSL" }));
  await user.type(screen.getByLabelText("ZeroSSL EAB key ID"), "test-id");
  await user.type(screen.getByLabelText("ZeroSSL EAB HMAC key"), "c2VjcmV0");
  await user.click(screen.getByRole("button", { name: "Save HTTPS settings" }));
  await waitFor(() =>
    expect(patches).toEqual([{ issuer: "zerossl", eab_kid: "test-id", eab_hmac_key: "c2VjcmV0" }]),
  );
  await waitFor(() => expect(screen.getByLabelText("ZeroSSL EAB HMAC key")).toHaveValue(""));
  expect(screen.getByLabelText("ZeroSSL EAB key ID")).toHaveValue("");
  expect(await screen.findByText(/Credentials are saved/)).toBeInTheDocument();
});
it("preserves stored ZeroSSL credentials when only saving the authority", async () => {
  const patches = setup(true);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Save HTTPS settings" }));
  await waitFor(() => expect(patches).toEqual([{ issuer: "zerossl" }]));
});
