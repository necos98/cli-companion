import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import { Section, useWorkspaceOp } from "./shared";

interface Props {
  workspace: Workspace;
  onWorkspaceUpdated: (ws: Workspace) => void;
}

/** Interruttore "Rispetta le fasce orarie" (`use_schedule`) del workspace. */
export default function UseScheduleSection({ workspace, onWorkspaceUpdated }: Props) {
  const { run, saving, error } = useWorkspaceOp(onWorkspaceUpdated);
  return (
    <Section>
      <label className="flex items-center gap-2 text-[13px]">
        <input
          type="checkbox"
          checked={workspace.use_schedule === true}
          disabled={saving}
          onChange={(e) => void run(() => api.setUseSchedule(workspace.id, e.target.checked))}
          className="h-3.5 w-3.5 accent-primary"
        />
        Rispetta le fasce orarie
      </label>
      <p className="text-[11px] text-muted-foreground">
        Con le fasce orarie attive (Impostazioni › Fasce orarie) la coda di questo workspace fa
        partire nuovi task solo dentro le fasce; quelli già in corso finiscono. Spento: la coda
        parte sempre.
      </p>
      {error && <p className="text-[11px] text-destructive">{error}</p>}
    </Section>
  );
}
