import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { expect, it } from "vite-plus/test";

import { BrandingProvider } from "@/features/branding/branding-context";
import { AuthLayout } from "./auth-layout";

it("shows the subdued product attribution and configured version", () => {
  render(
    <BrandingProvider branding={{ siteName: "Acme Deploy", version: "9.9.9" }}>
      <MemoryRouter initialEntries={["/login"]}>
        <Routes>
          <Route element={<AuthLayout />}>
            <Route path="/login" element={<div>Login</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </BrandingProvider>,
  );

  expect(screen.getByText("Powered by Grass Worker · v9.9.9")).toBeInTheDocument();
});
