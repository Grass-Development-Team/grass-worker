export type FrontendConfig = {
  apiBaseUrl: string;
  nodeBaseUrl: string;
};

export function renderHomePage(config: FrontendConfig): string {
  return `
    <main>
      <h1>Hello, World</h1>
      <p>grass-worker initial scaffold</p>
      <p>API: ${config.apiBaseUrl}</p>
      <p>Node: ${config.nodeBaseUrl}</p>
    </main>
  `;
}
