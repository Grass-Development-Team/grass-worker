import { request } from "./client";

export const adminUsersQueryKey = ["admin", "users"] as const;

export type User = {
  id: string;
  email: string;
  is_admin: boolean;
  is_initial_admin: boolean;
  created_at: string;
  updated_at: string;
};

type UsersEnvelope = {
  users: User[];
};

export async function getAdminUsers(): Promise<User[]> {
  const response = await request<UsersEnvelope>("/api/v1/admin/users");
  return response.users;
}
