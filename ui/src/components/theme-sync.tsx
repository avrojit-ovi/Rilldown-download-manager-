import { useEffect, useState } from "react";

import { useUiStore } from "@/stores/ui";

export function ThemeSync() {
  const theme = useUiStore((state) => state.theme);
  const [systemDark, setSystemDark] = useState(false);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") {
      return;
    }
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    setSystemDark(media.matches);
    const onChange = (event: MediaQueryListEvent) => setSystemDark(event.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    const dark = theme === "dark" || (theme === "system" && systemDark);
    document.documentElement.classList.toggle("dark", dark);
    document.documentElement.style.colorScheme = dark ? "dark" : "light";
  }, [theme, systemDark]);

  return null;
}
