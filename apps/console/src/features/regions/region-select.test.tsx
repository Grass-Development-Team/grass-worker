import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useState } from "react";
import { RegionSelect } from "./region-select";
import { regionsApi, type Region } from "./regions.api";

const regions: Region[] = [
  {
    code: "hk_1",
    name: "Hong Kong",
    ingress_hostname: "hk.entry.example.com",
    ingress_enabled: true,
  },
  { code: "hk_frick", name: "hk_frick" },
];
function setup(props: Partial<Parameters<typeof RegionSelect>[0]> = {}) {
  vi.spyOn(regionsApi, "list").mockResolvedValue({ regions });
  const onChange = vi.fn();
  function Form() {
    const [value, setValue] = useState("");
    return (
      <>
        <label htmlFor="test-region">Region</label>
        <RegionSelect
          id="test-region"
          value={value}
          onChange={(code) => {
            setValue(code);
            onChange(code);
          }}
          {...props}
        />
      </>
    );
  }
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <Form />
    </QueryClientProvider>,
  );
  return onChange;
}
afterEach(() => vi.restoreAllMocks());

it("selects existing region tags without changing underscores", async () => {
  const changed = setup({ allowCreate: true });
  const user = userEvent.setup();
  await waitFor(() => expect(screen.getByRole("combobox")).toBeEnabled());
  await user.click(screen.getByRole("combobox"));
  await user.click(screen.getByRole("option", { name: "hk_frick" }));
  expect(changed).toHaveBeenCalledWith("hk_frick");
});
it("creates and selects a region from the Node-style picker", async () => {
  const changed = setup({ allowCreate: true });
  vi.spyOn(regionsApi, "create").mockImplementation(async ({ code }) => {
    const region = { code, name: code };
    vi.mocked(regionsApi.list).mockResolvedValue({ regions: [...regions, region] });
    return { region };
  });
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "New Region" }));
  await user.type(screen.getByLabelText("Region code"), "hk_2");
  await user.click(screen.getByRole("button", { name: "Create region" }));
  await waitFor(() => expect(changed).toHaveBeenCalledWith("hk_2"));
  expect(screen.getByRole("combobox")).toHaveTextContent("hk_2");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
it("regional entries allow only existing, unused regions", async () => {
  setup({ unusedOnly: true });
  const user = userEvent.setup();
  expect(screen.queryByRole("button", { name: "New Region" })).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("combobox")).toBeEnabled());
  await user.click(screen.getByRole("combobox"));
  expect(screen.getByRole("option", { name: /Hong Kong/ })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  expect(screen.getByRole("option", { name: "hk_frick" })).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});
it("domain pickers explain regions without an enabled entry", async () => {
  setup({ requireIngress: true });
  const user = userEvent.setup();
  await waitFor(() => expect(screen.getByRole("combobox")).toBeEnabled());
  await user.click(screen.getByRole("combobox"));
  expect(screen.getByRole("option", { name: /hk_frick — No enabled entry/ })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
});
