import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { authApi, type AuthUser, type RegisterInput } from "./auth.api";
import { API_UNAUTHORIZED_EVENT } from "@/lib/api";
import { setCsrfToken } from "@/lib/csrf";

interface AuthState {
  user: AuthUser | null;
  isLoading: boolean;
  login: (email: string, password: string) => Promise<void>;
  register: (input: RegisterInput) => Promise<void>;
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

  const login = useCallback(async (email: string, password: string) => {
    const data = await authApi.login(email, password);
    setUser(data.user);
  }, []);

  const register = useCallback(async (input: RegisterInput) => {
    const data = await authApi.register(input);
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
    () => ({ user, isLoading, login, register, updateProfile, logout }),
    [user, isLoading, login, register, updateProfile, logout],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthContext);
  if (!ctx) {
    throw new Error("useAuth must be used within AuthProvider");
  }
  return ctx;
}
