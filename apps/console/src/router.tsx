import { Route, Routes } from "react-router";

import { DashboardRoute } from "@/routes/dashboard";
import { LoginRoute } from "@/routes/login";

export function Router() {
  return (
    <Routes>
      <Route path="/" element={<DashboardRoute />} />
      <Route path="/login" element={<LoginRoute />} />
      <Route path="/dashboard" element={<DashboardRoute />} />
    </Routes>
  );
}
