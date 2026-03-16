import { create } from "zustand";

type ThemeMode = "light" | "dark" | "system";
type ResolvedTheme = "light" | "dark";
type ThemeStore = {
  theme: ThemeMode;
  resolvedTheme: ResolvedTheme;
  initialized: boolean;
  initTheme: () => void;
  setTheme: (theme: ThemeMode) => void;
  toggleTheme: () => void;
};
const STORAGE_KEY = "rusuh-dashboard-theme";
const MEDIA_QUERY = "(prefers-color-scheme: dark)";

function resolveSystemTheme(): ResolvedTheme {
  if (typeof window === "undefined") {
    return "light";
  }

  return window.matchMedia(MEDIA_QUERY).matches ? "dark" : "light";
}

function resolveStoredTheme(): ThemeMode {
  if (typeof window === "undefined") {
    return "system";
  }
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (stored === "light" || stored === "dark" || stored === "system") {
    return stored;
  }

  return "system";
}

function resolveTheme(theme: ThemeMode): ResolvedTheme {
  return theme === "system" ? resolveSystemTheme() : theme;
}

function applyTheme(theme: ThemeMode) {
  if (typeof document === "undefined") {
    return resolveTheme(theme);
  }

  const resolvedTheme = resolveTheme(theme);
  document.documentElement.classList.toggle("dark", resolvedTheme === "dark");
  document.documentElement.style.colorScheme = resolvedTheme;
  return resolvedTheme;
}

let mediaQueryCleanupBound = false;

function bindSystemThemeListener() {
  if (mediaQueryCleanupBound || typeof window === "undefined") {
    return;
  }

  const mediaQuery = window.matchMedia(MEDIA_QUERY);
  const listener = () => {
    const state = useThemeStore.getState();
    if (state.theme !== "system") {
      return;
    }

    const resolvedTheme = applyTheme("system");
    useThemeStore.setState({ resolvedTheme, initialized: true });
  };

  mediaQuery.addEventListener("change", listener);
  mediaQueryCleanupBound = true;
}

export const useThemeStore = create<ThemeStore>((set) => ({
  theme: "system",
  resolvedTheme: "light",
  initialized: false,
  initTheme: () => {
    bindSystemThemeListener();
    const theme = resolveStoredTheme();
    const resolvedTheme = applyTheme(theme);
    set({ theme, resolvedTheme, initialized: true });
  },
  setTheme: (theme) => {
    const resolvedTheme = applyTheme(theme);
    if (typeof window !== "undefined") {
      window.localStorage.setItem(STORAGE_KEY, theme);
    }
    set({ theme, resolvedTheme, initialized: true });
  },
  toggleTheme: () =>
    set((state) => {
      const nextTheme = state.resolvedTheme === "dark" ? "light" : "dark";
      const resolvedTheme = applyTheme(nextTheme);
      if (typeof window !== "undefined") {
        window.localStorage.setItem(STORAGE_KEY, nextTheme);
      }
      return { theme: nextTheme, resolvedTheme, initialized: true };
    }),
}));
