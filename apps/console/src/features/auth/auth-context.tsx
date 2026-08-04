import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import {
  authApi,
  isAuthResponse,
  type AuthUser,
  type LoginResponse,
  type RegisterInput,
  type RegisterResponse,
} from "./auth.api";
import { API_UNAUTHORIZED_EVENT } from "@/lib/api";
import { setCsrfToken } from "@/lib/csrf";

interface AuthState {
  user: AuthUser | null;
  isLoading: boolean;
  login: (email: string, password: string, returnTo?: string) => Promise<LoginResponse>;
  register: (input: RegisterInput) => Promise<RegisterResponse>;
  completeMfa: (challengeToken: string, factorId: string, code: string) => Promise<void>;
  verifyEmail: (token: string) => Promise<void>;
  updateProfile: (displayName: string | null) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    let active = true;
    const clearAuthentication = () => {
      setUser(null);
      setCsrfToken(null);
    };
    window.addEventListener(API_UNAUTHORIZED_EVENT, clearAuthentication);

    authApi
      .restore()
      .then((data) => {
        if (active) setUser(data.user);
      })
      .catch(() => {
        if (active) clearAuthentication();
      })
      .finally(() => {
        if (active) setIsLoading(false);
      });

    return () => {
      active = false;
      window.removeEventListener(API_UNAUTHORIZED_EVENT, clearAuthentication);
    };
  }, []);

  const login = useCallback(async (email: string, password: string, returnTo?: string) => {
    const data = await authApi.login(email, password, returnTo);
    if (isAuthResponse(data)) setUser(data.user);
    return data;
  }, []);

  const register = useCallback(async (input: RegisterInput) => {
    const data = await authApi.register(input);
    if (isAuthResponse(data)) setUser(data.user);
    return data;
  }, []);

  const completeMfa = useCallback(
    async (challengeToken: string, factorId: string, code: string) => {
      const data = await authApi.mfaVerify(challengeToken, factorId, code);
      setUser(data.user);
    },
    [],
  );

  const verifyEmail = useCallback(async (token: string) => {
    const data = await authApi.verifyEmail(token);
    setUser(data.user);
  }, []);

  const logout = useCallback(async () => {
    await authApi.logout();
    setUser(null);
  }, []);

  const updateProfile = useCallback(async (displayName: string | null) => {
    const data = await authApi.updateMe({ display_name: displayName });
    setUser(data.user);
  }, []);

  const value = useMemo(
    () => ({
      user,
      isLoading,
      login,
      register,
      completeMfa,
      verifyEmail,
      updateProfile,
      logout,
    }),
    [user, isLoading, login, register, completeMfa, verifyEmail, updateProfile, logout],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
