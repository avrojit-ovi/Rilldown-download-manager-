import { useEffect, useState } from "react";

import { BROWSER_APP_INFO, getAppInfo, isTauri, type AppInfo } from "@/lib/tauri";

export function useAppInfo(): AppInfo | null {
  const [info, setInfo] = useState<AppInfo | null>(() =>
    isTauri() ? null : BROWSER_APP_INFO,
  );

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let active = true;
    getAppInfo()
      .then((value) => {
        if (active) {
          setInfo(value);
        }
      })
      .catch((error: unknown) => {
        console.warn("Rilldown identity unavailable", error);
      });
    return () => {
      active = false;
    };
  }, []);

  return info;
}
