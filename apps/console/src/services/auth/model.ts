export type PlatformRole = "admin" | "user";

export type MfaFactorKind = "totp" | "email";

export interface AuthUser {
  id: string;
  email: string;
  display_name: string | null;
  avatar_url: string | null;
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
  return_to?: string;
  registration_code?: string;
}

export interface MeResponse {
  user: AuthUser;
}

export interface CsrfResponse {
  csrf_token: string;
}

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
