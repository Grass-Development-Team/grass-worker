import { renderHomePage } from "./render";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("Missing #app root");
}

app.innerHTML = renderHomePage({
  apiBaseUrl: import.meta.env.VITE_API_BASE_URL ?? "http://127.0.0.1:3000",
  nodeBaseUrl: import.meta.env.VITE_NODE_BASE_URL ?? "http://127.0.0.1:3001",
});
