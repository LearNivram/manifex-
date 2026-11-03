import { useEffect, useState } from "react";
import { useApp } from "@/store/app";

const media = typeof window !== "undefined" ? window.matchMedia("(prefers-color-scheme: dark)") : null;

/** Resolves the effective theme ("system" follows the OS) and applies it to <html>. */
export function useResolvedTheme(): "dark" | "light" {
  const pref = useApp((s) => s.settings?.theme ?? "system");
  const [systemDark, setSystemDark] = useState(media?.matches ?? true);
  useEffect(() => {
    if (!media) return;
    const l = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    media.addEventListener("change", l);
    return () => media.removeEventListener("change", l);
  }, []);
  return pref === "system" ? (systemDark ? "dark" : "light") : pref;
}

export function useApplyTheme() {
  const theme = useResolvedTheme();
  const density = useApp((s) => s.settings?.density ?? "comfortable");
  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", theme === "dark");
    root.classList.remove("density-compact", "density-comfortable");
    root.classList.add(`density-${density}`);
  }, [theme, density]);
  return theme;
}
