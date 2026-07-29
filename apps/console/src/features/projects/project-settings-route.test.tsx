import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { it, vi } from "vitest";

import { useProject } from "./project-layout";
import { ProjectSettingsRoute } from "./project-settings-route";

vi.mock("./project-layout", () => ({ useProject: vi.fn() }));

function renderSettings(role: "admin" | "member" | "viewer") {
  vi.mocked(useProject).mockReturnValue({
    role,
    project: {
      id: "project-1",
      team_id: "team-1",
      name: "Website",
      slug: "website",
      archived_at: null,
    },
  } as ReturnType<typeof useProject>);
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  return render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <ProjectSettingsRoute />
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

it("shows project settings read-only to viewers", () => {
  renderSettings("viewer");

  expect(screen.getByLabelText("Project name")).toHaveAttribute("readonly");
  expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Archive" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Delete" })).not.toBeInTheDocument();
  expect(screen.getByLabelText("Project ID")).toHaveValue("project-1");
});

it("separates member edits from administrator lifecycle actions", () => {
  const { unmount } = renderSettings("member");
  expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Archive" })).not.toBeInTheDocument();
  unmount();

  renderSettings("admin");
  expect(screen.getByRole("button", { name: "Archive" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
});
