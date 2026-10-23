import { useEffect, useState } from "react";
import { api } from "../../lib/tauri";

/** Repository rilevati del workspace (`workspace_detect_repos`); vuoto finché non risponde o se fallisce. */
export function useWorkspaceRepos(workspaceId: string): string[] {
  const [repos, setRepos] = useState<string[]>([]);
  useEffect(() => {
    let cancelled = false;
    api
      .workspaceDetectRepos(workspaceId)
      .then((r) => {
        if (!cancelled) setRepos(r);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [workspaceId]);
  return repos;
}
