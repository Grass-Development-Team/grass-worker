import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { setupApi } from "@/features/setup/setup.api";
import { Router } from "./router";

export function App() {
  const navigate = useNavigate();

  const { data: setupState } = useQuery({
    queryKey: ["setup-state-bootstrap"],
    queryFn: setupApi.getSetupState,
    retry: true,
    retryDelay: 2000,
  });

  if (setupState && setupState.stage !== "complete" && window.location.pathname !== "/setup") {
    navigate("/setup", { replace: true });
    return null;
  }

  if (setupState?.stage === "complete" && ["/setup", "/"].includes(window.location.pathname)) {
    navigate("/login", { replace: true });
    return null;
  }

  return <Router />;
}
