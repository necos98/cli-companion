import { useState } from "react";
import { Button } from "../ui/button";
import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import { cn } from "../../lib/utils";
import { Section, SettingsPage, useWorkspaceOp } from "./shared";

interface Props {
  /** Solo i workspace archiviati. */
  workspaces: Workspace[];
  onWorkspaceUpdated: (ws: Workspace) => void;
  onRemove: (id: string) => Promise<void>;
}

/** Elenco dei workspace archiviati: ripristino o eliminazione. */
export default function ArchivedPage({ workspaces, onWorkspaceUpdated, onRemove }: Props) {
  const { run, saving, error, setError } = useWorkspaceOp(onWorkspaceUpdated);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);

  const remove = async (id: string) => {
    setRemoving(true);
    setError(null);
    try {
      await onRemove(id);
      setConfirmingId(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setRemoving(false);
    }
  };

  return (
    <SettingsPage
      title="Workspace archiviati"
      description="Nascosti dalla sidebar, con la coda ferma. Ripristinali per riaverli."
    >
      {error && <p className="text-xs text-destructive">{error}</p>}
      {workspaces.length === 0 ? (
        <p className="text-[13px] text-muted-foreground">Nessun workspace archiviato.</p>
      ) : (
        <Section className="gap-0 divide-y p-0">
          {workspaces.map((ws) => {
            const wsl = ws.kind === "wsl";
            return (
              <div key={ws.id} className="flex flex-col gap-1.5 px-3 py-2.5">
                <div className="flex items-center gap-2">
                  <span
                    className={cn(
                      "flex-shrink-0 rounded border px-1 py-px font-mono text-[10px] font-medium",
                      wsl
                        ? "border-primary/40 text-primary"
                        : "border-border-strong text-muted-foreground",
                    )}
                  >
                    {wsl ? "WSL" : "WIN"}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-[13px] font-medium">
                    {ws.name}
                  </span>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={saving || removing}
                    onClick={() => {
                      setConfirmingId(null);
                      void run(() => api.setWorkspaceArchived(ws.id, false));
                    }}
                  >
                    Ripristina
                  </Button>
                  <Button
                    size="sm"
                    variant="destructive"
                    disabled={saving || removing}
                    onClick={() => {
                      setError(null);
                      setConfirmingId(ws.id);
                    }}
                  >
                    Elimina
                  </Button>
                </div>
                <p
                  className="truncate font-mono text-[11px] text-muted-foreground"
                  title={ws.path}
                >
                  {wsl && ws.distro ? `${ws.distro} · ${ws.path}` : ws.path}
                </p>
                {confirmingId === ws.id && (
                  <div className="flex items-center gap-1.5">
                    <span className="flex-1 text-xs">Rimuovere?</span>
                    <Button
                      size="sm"
                      variant="destructive"
                      disabled={removing}
                      onClick={() => void remove(ws.id)}
                    >
                      Sì
                    </Button>
                    <Button size="sm" variant="secondary" onClick={() => setConfirmingId(null)}>
                      No
                    </Button>
                  </div>
                )}
              </div>
            );
          })}
        </Section>
      )}
    </SettingsPage>
  );
}
