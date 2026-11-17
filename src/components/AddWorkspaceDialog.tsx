import { useEffect, useState } from "react";
import { Button } from "./ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "./ui/dialog";
import { Input } from "./ui/input";
import { api } from "../lib/tauri";
import type { Workspace } from "../lib/tauri";
import { LocationKindToggle, LocationPathFields } from "./workspace-location/LocationFields";
import { useWorkspaceLocation } from "./workspace-location/useWorkspaceLocation";

interface Props {
  isOpen: boolean;
  onClose: () => void;
  onAdded: (ws: Workspace) => void;
}

export default function AddWorkspaceDialog({ isOpen, onClose, onAdded }: Props) {
  const [name, setName] = useState("");
  const [shell, setShell] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loc = useWorkspaceLocation(isOpen, setError);

  useEffect(() => {
    if (!isOpen) setError(null);
  }, [isOpen]);

  const save = async () => {
    setError(null);
    const choice = loc.choose();
    if (!choice.ok) {
      setError(choice.error);
      return;
    }
    setSaving(true);
    try {
      const ws = await api.addWorkspace({ name: name.trim(), ...choice.location, shell: shell.trim() || undefined });
      onAdded(ws);
      onClose();
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
        if (!o) onClose();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Nuovo workspace</DialogTitle>
        </DialogHeader>

        <DialogBody className="gap-3">
          <LocationKindToggle loc={loc} />

          <div>
            <p className="mb-1 text-xs font-semibold">Nome (opzionale)</p>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Default: nome cartella" />
          </div>

          <LocationPathFields loc={loc} />

          <div>
            <p className="mb-1 text-xs font-semibold">Shell (opzionale)</p>
            <Input
              value={shell}
              onChange={(e) => setShell(e.target.value)}
              placeholder={loc.fields.kind === "windows" ? "powershell.exe" : "default della distro"}
            />
          </div>

          {error && <p className="text-xs text-destructive">{error}</p>}
        </DialogBody>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
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
