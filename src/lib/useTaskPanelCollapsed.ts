import { useCallback, useState } from "react";
import { readCollapsed, withCollapsed, writeCollapsed } from "./taskPanelPrefs";
import type { CollapsedMap } from "./taskPanelPrefs";

function browserStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * Stato compresso/espanso del pannello Task per workspace (o per la vista
 * Globale, con il suo id riservato), ricordato in localStorage.
 */
export function useTaskPanelCollapsed() {
  const [map, setMap] = useState<CollapsedMap>(() => readCollapsed(browserStorage()));

  const isCollapsed = useCallback((id: string) => map[id] === true, [map]);

  const toggle = useCallback((id: string) => {
    setMap((prev) => {
      const next = withCollapsed(prev, id, prev[id] !== true);
      writeCollapsed(browserStorage(), next);
      return next;
    });
  }, []);

  return { isCollapsed, toggle };
}
