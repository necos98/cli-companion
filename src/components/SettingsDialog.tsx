import { useEffect, useState } from "react";
import { Pencil, Star, Trash2 } from "lucide-react";
import { Button } from "./ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { Input } from "./ui/input";
import AgentSettingsTab from "./AgentSettingsTab";
import { api } from "../lib/tauri";
import type { Launch, Workspace } from "../lib/tauri";
import {
  LAUNCH_COMMAND_MAX,
  LAUNCH_NAME_MAX,
  sanitizeLaunchCommand,
} from "../lib/launch";
import { cn } from "../lib/utils";

export type SettingsTab = "launches" | "agents";

interface Props {
  isOpen: boolean;
  initialTab?: SettingsTab;
  /** Workspace preselezionato nel tab degli avvii. */
  initialWorkspaceId?: string;
  workspaces: Workspace[];
  onClose: () => void;
  onWorkspaceUpdated: (ws: Workspace) => void;
}

const selectClass =
  "flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 [&>option]:bg-popover [&>option]:text-popover-foreground";

export default function SettingsDialog({
  isOpen,
  initialTab = "launches",
  initialWorkspaceId,
  workspaces,
  onClose,
  onWorkspaceUpdated,
}: Props) {
  const [tab, setTab] = useState<SettingsTab>(initialTab);
  const [workspaceId, setWorkspaceId] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Form di aggiunta.
  const [newName, setNewName] = useState("");
  const [newCommand, setNewCommand] = useState("");
  const [newDefault, setNewDefault] = useState(false);

  // Modifica inline di un avvio esistente.
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editCommand, setEditCommand] = useState("");

  // All'apertura: tab e workspace richiesti, form puliti.
  useEffect(() => {
    if (!isOpen) return;
    setTab(initialTab);
    setWorkspaceId(initialWorkspaceId ?? workspaces[0]?.id ?? "");
    setError(null);
    setNewName("");
    setNewCommand("");
    setNewDefault(false);
    setEditingId(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, initialTab, initialWorkspaceId]);

  // Se il workspace selezionato sparisce, ripiega sul primo disponibile.
  const selected =
    workspaces.find((w) => w.id === workspaceId) ?? workspaces[0] ?? undefined;

  const run = async (op: () => Promise<Workspace>): Promise<boolean> => {
    setError(null);
    setSaving(true);
    try {
      onWorkspaceUpdated(await op());
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const addLaunch = async () => {
    if (!selected) return;
    const ok = await run(() =>
      api.addLaunch(selected.id, newName, sanitizeLaunchCommand(newCommand), newDefault),
    );
    if (ok) {
      setNewName("");
      setNewCommand("");
      setNewDefault(false);
    }
  };

  const startEdit = (l: Launch) => {
    setError(null);
    setEditingId(l.id);
    setEditName(l.name);
    setEditCommand(l.command);
  };

  const saveEdit = async (l: Launch) => {
    if (!selected) return;
    const ok = await run(() =>
      api.updateLaunch(
        selected.id,
        l.id,
        editName,
        sanitizeLaunchCommand(editCommand),
        l.is_default,
      ),
    );
    if (ok) setEditingId(null);
  };

  const toggleDefault = (l: Launch) => {
    if (!selected) return;
    void run(() =>
      api.updateLaunch(selected.id, l.id, l.name, l.command, !l.is_default),
    );
  };

  const removeLaunch = (l: Launch) => {
    if (!selected) return;
    void run(() => api.removeLaunch(selected.id, l.id));
  };

  const canAdd =
    !saving && newName.trim() !== "" && sanitizeLaunchCommand(newCommand) !== "";

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <DialogContent className="sm:max-w-[560px]">
        <DialogHeader>
          <DialogTitle>Impostazioni</DialogTitle>
        </DialogHeader>

        <div className="flex gap-1 rounded-lg border bg-muted/50 p-1">
          {(
            [
              { id: "launches", label: "Avvii personalizzati" },
              { id: "agents", label: "Coding agent" },
            ] as const
          ).map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={cn(
                "flex-1 rounded-md px-2 py-1 text-xs font-medium",
                tab === t.id
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {t.label}
            </button>
          ))}
        </div>

        {tab === "agents" ? (
          <AgentSettingsTab />
        ) : workspaces.length === 0 || !selected ? (
          <p className="text-[13px] text-muted-foreground">
            Nessun workspace. Aggiungi una cartella per definire i suoi avvii.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            <div>
              <p className="mb-1 text-xs font-semibold">Workspace</p>
              <select
                value={selected.id}
                onChange={(e) => {
                  setWorkspaceId(e.target.value);
                  setEditingId(null);
                  setError(null);
                }}
                className={selectClass}
              >
                {workspaces.map((w) => (
                  <option key={w.id} value={w.id}>
                    {w.name}
                  </option>
                ))}
              </select>
              <p className="mt-1 text-[11px] text-muted-foreground">
                Gli avvii compaiono nel menu “⋯” del workspace. Quello
                predefinito parte con “+ Terminale”.
              </p>
            </div>

            <div className="flex max-h-[240px] flex-col gap-2 overflow-y-auto">
              {selected.launches.length === 0 && (
                <p className="text-xs text-muted-foreground">
                  Nessun avvio: “+ Terminale” aprirà una shell vuota.
                </p>
              )}
              {selected.launches.map((l) =>
                editingId === l.id ? (
                  <div key={l.id} className="flex flex-col gap-2 rounded-lg border bg-card p-2.5">
                    <Input
                      value={editName}
                      onChange={(e) => setEditName(e.target.value)}
                      placeholder="Nome"
                      maxLength={LAUNCH_NAME_MAX}
                      className="h-8 text-[13px]"
                      autoFocus
                    />
                    <Input
                      value={editCommand}
                      onChange={(e) => setEditCommand(e.target.value)}
                      placeholder="Comando"
                      maxLength={LAUNCH_COMMAND_MAX}
                      className="h-8 font-mono text-[12px]"
                    />
                    <div className="flex flex-row gap-1.5">
                      <Button
                        size="sm"
                        disabled={saving}
                        onClick={() => void saveEdit(l)}
                      >
                        Salva
                      </Button>
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => setEditingId(null)}
                      >
                        Annulla
                      </Button>
                    </div>
                  </div>
                ) : (
                  <div
                    key={l.id}
                    className="flex items-center gap-2 rounded-lg border bg-card p-2.5"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-[13px] font-semibold">
                        {l.name}
                        {l.is_default && (
                          <span className="ml-2 text-[10px] font-normal text-muted-foreground">
                            predefinito
                          </span>
                        )}
                      </p>
                      <code className="block truncate font-mono text-[11px] text-muted-foreground">
                        {l.command}
                      </code>
                    </div>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="h-7 w-7"
                      disabled={saving}
                      title={l.is_default ? "Rimuovi predefinito" : "Imposta predefinito"}
                      aria-label={l.is_default ? "Rimuovi predefinito" : "Imposta predefinito"}
                      aria-pressed={l.is_default}
                      onClick={() => toggleDefault(l)}
                    >
                      <Star className={cn(l.is_default && "fill-current")} />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="h-7 w-7"
                      title="Modifica"
                      aria-label="Modifica"
                      onClick={() => startEdit(l)}
                    >
                      <Pencil />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="h-7 w-7"
                      disabled={saving}
                      title="Elimina"
                      aria-label="Elimina"
                      onClick={() => removeLaunch(l)}
                    >
                      <Trash2 />
                    </Button>
                  </div>
                ),
              )}
            </div>

            <div className="flex flex-col gap-2 rounded-lg border p-2.5">
              <p className="text-xs font-semibold">Nuovo avvio</p>
              <Input
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="Nome (es. Dev server)"
                maxLength={LAUNCH_NAME_MAX}
                className="h-8 text-[13px]"
              />
              <Input
                value={newCommand}
                onChange={(e) => setNewCommand(e.target.value)}
                placeholder="Comando (es. npm run dev)"
                maxLength={LAUNCH_COMMAND_MAX}
                className="h-8 font-mono text-[12px]"
              />
              <label className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={newDefault}
                  onChange={(e) => setNewDefault(e.target.checked)}
                  className="h-3.5 w-3.5 accent-primary"
                />
                Predefinito
              </label>
              <div>
                <Button size="sm" disabled={!canAdd} onClick={() => void addLaunch()}>
                  Aggiungi
                </Button>
              </div>
            </div>

            {error && <p className="text-xs text-destructive">{error}</p>}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Chiudi
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
