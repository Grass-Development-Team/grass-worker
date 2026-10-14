import { defineConfig, loadEnv } from "vite";
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  const target = env.VITE_API_TARGET ?? "http://127.0.0.1:7817";
  return {
    resolve: { alias: { "@": new URL("./src", import.meta.url).pathname } },
    server: {
      proxy: {
        "/api": { target, changeOrigin: true, ws: true },
        "/health": { target, changeOrigin: true },
      },
    },
    build: { target: "es2022" },
  };
});
