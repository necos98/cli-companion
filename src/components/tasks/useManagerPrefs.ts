import { useCallback, useState } from "react";
import { browserStorage } from "../../lib/browserStorage";
import { readPrefs, writePrefs } from "../../lib/taskViews";
import type { ListPrefs, ManagerView, WorkspacePrefs } from "../../lib/taskViews";

/** Vista e filtri della Lista del workspace, ricordati in localStorage. */
export function useManagerPrefs(workspaceId: string) {
  const [prefs, setPrefs] = useState<WorkspacePrefs>(() => readPrefs(browserStorage(), workspaceId));

  const update = useCallback(
    (change: (p: WorkspacePrefs) => WorkspacePrefs) => {
      setPrefs((prev) => {
        const next = change(prev);
        writePrefs(browserStorage(), workspaceId, next);
        return next;
      });
    },
    [workspaceId],
  );

  const setView = useCallback((view: ManagerView) => update((p) => ({ ...p, view })), [update]);
  const setList = useCallback((list: ListPrefs) => update((p) => ({ ...p, list })), [update]);
  return { prefs, setView, setList };
}
