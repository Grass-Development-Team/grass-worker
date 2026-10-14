import type { SourceCredential } from "@/services/teams";
export function credentialMatchesRepository(
  credential: Pick<SourceCredential, "kind" | "host" | "port">,
  repositoryUrl: string | null,
) {
  if (!repositoryUrl) return false;
  const value = repositoryUrl.trim();
  if (!value.includes("://")) {
    const match = /^(?:[^@]+@)?(?:\[([^\]]+)\]|([^:]+)):(.+)$/.exec(value);
    const host = (match?.[1] ?? match?.[2])?.toLowerCase().replace(/\.$/, "");
    return credential.kind === "ssh" && credential.port === 22 && credential.host === host;
  }
  try {
    const parsed = new URL(value);
    const kind = parsed.protocol === "https:" ? "https" : parsed.protocol === "ssh:" ? "ssh" : null;
    if (!kind || credential.kind !== kind) return false;
    const port = parsed.port ? Number(parsed.port) : kind === "https" ? 443 : 22;
    return (
      credential.host === parsed.hostname.toLowerCase().replace(/\.$/, "") &&
      credential.port === port
    );
  } catch {
    return false;
  }
}
