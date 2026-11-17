import { useEffect, useState } from "react";
import { Button } from "./ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "./ui/dialog";
import { useStore } from "../hooks/useStore";
import { closeWorkspaceLocation } from "../app/actions/settings";
import { changeWorkspaceLocation } from "../app/actions/workspaces";
import { locationOf, resetsShell } from "../lib/workspaceLocation";
import { uiStore } from "../stores/ui";
import { workspacesStore } from "../stores/workspaces";
import { LocationKindToggle, LocationPathFields } from "./workspace-location/LocationFields";
import { useWorkspaceLocation } from "./workspace-location/useWorkspaceLocation";

/** "Cambia cartella" dal menu ⋯ del workspace: precompilato con la posizione attuale. */
export default function WorkspaceLocationDialog() {
  const id = useStore(uiStore, (s) => s.locationWorkspaceId);
  const ws = useStore(workspacesStore, (s) => (id ? s.list.find((w) => w.id === id) : undefined));
  const isOpen = ws !== undefined;
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loc = useWorkspaceLocation(isOpen, setError);
  const { reset } = loc;

  // Si riparte dalla posizione attuale a ogni apertura (non a ogni aggiornamento del workspace).
  const wsId = ws?.id;
  useEffect(() => {
    if (!wsId) return;
    const current = workspacesStore.getState().list.find((w) => w.id === wsId);
    if (current) reset(locationOf(current));
    setError(null);
  }, [wsId, reset]);

  if (!ws) return null;

  const save = async () => {
    setError(null);
    const choice = loc.choose();
    if (!choice.ok) {
      setError(choice.error);
      return;
    }
    setSaving(true);
    try {
      await changeWorkspaceLocation(ws.id, choice.location);
      closeWorkspaceLocation();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(o) => {
        if (!o) closeWorkspaceLocation();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Cambia cartella di {ws.name}</DialogTitle>
        </DialogHeader>

        <DialogBody className="gap-3">
          <LocationKindToggle loc={loc} />
          <LocationPathFields loc={loc} />

          <p className="text-xs text-muted-foreground">
            Nessun file viene spostato. I terminali già aperti restano sulla cartella attuale; i nuovi partono dalla
            nuova.
          </p>
          {resetsShell(ws, loc.fields.kind) && (
            <p className="text-xs text-muted-foreground">
              Cambiando ambiente la shell personalizzata ({ws.shell}) torna quella predefinita.
            </p>
          )}

          {error && <p className="text-xs text-destructive">{error}</p>}
        </DialogBody>

        <DialogFooter>
          <Button variant="outline" onClick={closeWorkspaceLocation}>
            Annulla
          </Button>
          <Button disabled={saving} onClick={() => void save()}>
            {saving ? "Salvataggio…" : "Salva"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
