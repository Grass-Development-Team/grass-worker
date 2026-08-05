import { request } from "@/lib/api";
import { setCsrfToken } from "@/lib/csrf";

export type PlatformRole = "admin" | "user";
export type MfaFactorKind = "totp" | "email";

export interface AuthUser {
  id: string;
  email: string;
  display_name: string | null;
  platform_role: PlatformRole;
  email_verified: boolean;
}

export interface PasswordPolicy {
  min_length: number;
  max_length: number;
  require_lowercase: boolean;
  require_uppercase: boolean;
  require_number: boolean;
  require_symbol: boolean;
  history_count: number;
}

export interface MfaFactor {
  id: string;
  kind: MfaFactorKind;
  label?: string | null;
  verified: boolean;
  created_at: string;
  last_used_at: string | null;
}

export interface MfaChallenge {
  mfa_required: boolean;
  mfa_enrollment_required: boolean;
  challenge_token: string;
  factors: MfaFactor[];
  allowed_factors: MfaFactorKind[];
  return_to: string;
}

export interface AuthConfiguration {
  providers: Array<{ slug: string; name: string; kind: "oidc" | "github" }>;
  password_recovery_available: boolean;
  registration_email_verification: boolean;
  signup_policy: "open" | "invite_only" | "closed";
  password_policy: PasswordPolicy;
}

export interface AuthResponse {
  user: AuthUser;
  csrf_token: string;
}

export interface RegistrationVerificationResponse {
  verification_required: true;
  email: string;
}

export type LoginResponse = AuthResponse | MfaChallenge;
export type RegisterResponse = AuthResponse | RegistrationVerificationResponse;

export interface RegisterInput {
  email: string;
  display_name: string;
  password: string;
  invitation_token?: string;
  registration_code?: string;
}

interface MeResponse {
  user: AuthUser;
}

interface CsrfResponse {
  csrf_token: string;
}

export function isAuthResponse(value: LoginResponse | RegisterResponse): value is AuthResponse {
  return "user" in value && "csrf_token" in value;
}

function storeAuthenticatedResponse(data: AuthResponse): AuthResponse {
  setCsrfToken(data.csrf_token);
  return data;
}

let restorePromise: Promise<MeResponse> | null = null;

function restoreSession(): Promise<MeResponse> {
  if (!restorePromise) {
    const pending = (async () => {
      const data = await authApi.me();
      await authApi.csrf();
      return data;
    })();
    const shared = pending.finally(() => {
      if (restorePromise === shared) restorePromise = null;
    });
    restorePromise = shared;
  }
  return restorePromise;
}

export const authApi = {
  configuration: () => request<AuthConfiguration>("/api/v1/auth/providers"),
  login: async (email: string, password: string, returnTo?: string) => {
    const data = await request<LoginResponse>("/api/v1/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password, ...(returnTo ? { return_to: returnTo } : {}) }),
      credentials: "include" as RequestCredentials,
    });
    return isAuthResponse(data) ? storeAuthenticatedResponse(data) : data;
  },
  register: async (input: RegisterInput) => {
    const data = await request<RegisterResponse>("/api/v1/auth/register", {
      method: "POST",
      body: JSON.stringify(input),
      credentials: "include" as RequestCredentials,
    });
    return isAuthResponse(data) ? storeAuthenticatedResponse(data) : data;
  },
  logout: async () => {
    const data = await request<{ message: string }>("/api/v1/auth/logout", {
      method: "POST",
      credentials: "include" as RequestCredentials,
    });
    setCsrfToken(null);
    return data;
  },
  me: () =>
    request<MeResponse>("/api/v1/me", {
      credentials: "include" as RequestCredentials,
    }),
  updateMe: (input: { display_name: string | null }) =>
    request<MeResponse>("/api/v1/me", {
      method: "PATCH",
      body: JSON.stringify(input),
    }),
  csrf: async () => {
    const data = await request<CsrfResponse>("/api/v1/auth/csrf", {
      credentials: "include" as RequestCredentials,
    });
    setCsrfToken(data.csrf_token);
    return data;
  },
  forgotPassword: (email: string) =>
    request<{ accepted: true }>("/api/v1/auth/password/forgot", {
      method: "POST",
      body: JSON.stringify({ email }),
    }),
  resetPassword: (token: string, password: string) =>
    request<{ reset: true }>("/api/v1/auth/password/reset", {
      method: "POST",
      body: JSON.stringify({ token, password }),
    }),
  verifyEmail: async (token: string) =>
    storeAuthenticatedResponse(
      await request<AuthResponse>("/api/v1/auth/email/verify", {
        method: "POST",
        body: JSON.stringify({ token }),
        credentials: "include" as RequestCredentials,
      }),
    ),
  resendVerification: (email: string) =>
    request<{ accepted: true }>("/api/v1/auth/email/resend", {
      method: "POST",
      body: JSON.stringify({ email }),
    }),
  mfaChallenge: (challengeToken: string) =>
    request<MfaChallenge>("/api/v1/auth/mfa/challenge", {
      method: "POST",
      body: JSON.stringify({ challenge_token: challengeToken }),
    }),
  mfaTotpStart: (challengeToken: string) =>
    request<TotpEnrollment>("/api/v1/auth/mfa/totp/start", {
      method: "POST",
      body: JSON.stringify({ challenge_token: challengeToken }),
    }),
  mfaEmailSend: (challengeToken: string, factorId?: string) =>
    request<{ factor: MfaFactor }>("/api/v1/auth/mfa/email/send", {
      method: "POST",
      body: JSON.stringify({
        challenge_token: challengeToken,
        ...(factorId ? { factor_id: factorId } : {}),
      }),
    }),
  mfaVerify: async (challengeToken: string, factorId: string, code: string) => {
    const data = await request<LoginResponse>("/api/v1/auth/mfa/verify", {
      method: "POST",
      body: JSON.stringify({ challenge_token: challengeToken, factor_id: factorId, code }),
      credentials: "include" as RequestCredentials,
    });
    return isAuthResponse(data) ? storeAuthenticatedResponse(data) : data;
  },
  security: () => request<AccountSecurity>("/api/v1/me/security"),
  changePassword: (currentPassword: string, password: string) =>
    request<{ changed: true }>("/api/v1/me/password", {
      method: "POST",
      body: JSON.stringify({ current_password: currentPassword, password }),
    }),
  accountTotpStart: () => request<TotpEnrollment>("/api/v1/me/mfa/totp/start", { method: "POST" }),
  accountEmailStart: () =>
    request<{ factor: MfaFactor }>("/api/v1/me/mfa/email/start", { method: "POST" }),
  accountMfaConfirm: (factorId: string, code: string) =>
    request<{ factor: MfaFactor }>(`/api/v1/me/mfa/${factorId}/confirm`, {
      method: "POST",
      body: JSON.stringify({ code }),
    }),
  accountMfaDelete: (factorId: string) =>
    request<{ deleted: true }>(`/api/v1/me/mfa/${factorId}`, { method: "DELETE" }),
  restore: restoreSession,
};

export interface TotpEnrollment {
  factor: MfaFactor;
  secret: string;
  otpauth_uri: string;
}

export interface AccountSecurity {
  email_verified: boolean;
  factors: MfaFactor[];
  allowed_factors: MfaFactorKind[];
  mfa_required: boolean;
  mfa_requirements: {
    minimum_factors: number;
    required_factors: MfaFactorKind[];
  };
  mfa_policy: {
    inherit_platform: boolean;
    minimum_factors: number;
    required_factors: MfaFactorKind[];
  };
  password_policy: PasswordPolicy;
  mail_available: boolean;
}
