import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { teamsApi } from "./teams.api";

const response = (data: unknown) =>
  new Response(JSON.stringify({ code: 200, message: "ok", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });

describe("teamsApi", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("lists teams for the current user", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({
        teams: [
          {
            id: "team-1",
            slug: "personal",
            name: "Personal",
            kind: "personal",
            owner_user_id: "user-1",
            group_id: "group-1",
          },
        ],
      }),
    );

    await expect(teamsApi.list()).resolves.toMatchObject({
      teams: [{ id: "team-1", kind: "personal" }],
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/teams",
      expect.objectContaining({ credentials: "include" }),
    );
  });

  it("creates a team with a normalized payload", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({
        team: {
          id: "team-2",
          slug: "acme",
          name: "Acme",
          kind: "team",
          owner_user_id: "user-1",
          group_id: null,
        },
      }),
    );

    await teamsApi.create({ name: "Acme", slug: "acme" });

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/teams",
      expect.objectContaining({
        method: "POST",
        credentials: "include",
        body: JSON.stringify({ name: "Acme", slug: "acme" }),
      }),
    );
  });

  it("uploads and removes a team avatar through the scoped endpoint", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(async () => response({ team: { id: "team-1", avatar_url: null } }));
    const png = new Blob(["png"], { type: "image/png" });

    await teamsApi.uploadAvatar("team-1", png);
    await teamsApi.deleteAvatar("team-1");

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "/api/v1/teams/team-1/avatar",
      expect.objectContaining({
        method: "PUT",
        body: png,
        credentials: "include",
        headers: expect.objectContaining({ "content-type": "image/png" }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "/api/v1/teams/team-1/avatar",
      expect.objectContaining({ method: "DELETE", credentials: "include" }),
    );
  });

  it("updates a member role through the scoped team endpoint", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        response({ member: { id: "member-1", user_id: "user-2", role: "admin" } }),
      );

    await teamsApi.updateMemberRole("team-1", "user-2", "admin");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/teams/team-1/members/user-2",
      expect.objectContaining({
        method: "PATCH",
        credentials: "include",
        body: JSON.stringify({ role: "admin" }),
      }),
    );
  });

  it("searches invitation candidates through the scoped team endpoint", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({
        candidates: [
          {
            kind: "user",
            user_id: "user-2",
            email: "alice@example.com",
            display_name: "Alice",
          },
        ],
      }),
    );

    await teamsApi.invitationCandidates("team-1", "Alice + email@example.com");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/teams/team-1/invitation-candidates?q=Alice+%2B+email%40example.com",
      expect.objectContaining({ credentials: "include" }),
    );
  });

  it("creates private source credentials through the team-scoped endpoint", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({
        credential: {
          id: "credential-1",
          team_id: "team-1",
          name: "deploy",
          kind: "https",
          host: "example.com",
          port: 443,
          username: "git",
          current_version_id: "version-1",
          revoked_at: null,
        },
      }),
    );

    const input = {
      name: "deploy",
      repository_url: "https://example.com/acme/repo.git",
      username: "git",
      secret: "write-only-token",
    };
    await teamsApi.createSourceCredential("team-1", input);

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/teams/team-1/source-credentials",
      expect.objectContaining({
        method: "POST",
        credentials: "include",
        body: JSON.stringify(input),
      }),
    );
  });
});
