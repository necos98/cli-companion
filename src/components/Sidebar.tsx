import { useState } from "react";
import { Ellipsis, FolderOpen, Plus, Star } from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { Input } from "./ui/input";
import { api } from "../lib/tauri";
import { cn } from "../lib/utils";
import type { Launch, Workspace } from "../lib/tauri";
import { defaultLaunch } from "../lib/launch";

interface Props {
  workspaces: Workspace[];
  /** Workspace del gruppo di terminali attivo (card evidenziata). */
  activeWorkspaceId: string | null;
  /** Click sulla card (non sui suoi pulsanti): seleziona il gruppo, se ha terminali. */
  onSelect: (id: string) => void;
  /** Apre un terminale; senza `launch` esegue l'avvio predefinito (se c'è). */
  onSpawn: (ws: Workspace, launch?: Launch) => void;
  /** Apre le impostazioni sugli avvii del workspace. */
  onManageLaunches: (ws: Workspace) => void;
  onRename: (id: string, name: string) => Promise<void>;
  onRemove: (id: string) => Promise<void>;
  onAdd: () => void;
}

export default function Sidebar({
  workspaces,
  activeWorkspaceId,
  onSelect,
  onSpawn,
  onManageLaunches,
  onRename,
  onRemove,
  onAdd,
}: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const startEdit = (ws: Workspace) => {
    setError(null);
    setConfirmingId(null);
    setEditingId(ws.id);
    setEditingName(ws.name);
  };

  const saveEdit = async (id: string) => {
    setBusyId(id);
    setError(null);
    try {
      await onRename(id, editingName);
      setEditingId(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  };

  const openFolder = (id: string) => {
    setError(null);
    api.openWorkspaceFolder(id).catch((e) => setError(String(e)));
  };

  const confirmRemove = async (id: string) => {
    setBusyId(id);
    setError(null);
    try {
      await onRemove(id);
      setConfirmingId(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="flex flex-1 flex-col gap-2 overflow-hidden p-3">
      <Button onClick={onAdd} className="w-full">
        <Plus />
        Aggiungi workspace
      </Button>

      {error && <p className="text-xs text-destructive">{error}</p>}

      <div className="flex flex-1 flex-col gap-2 overflow-y-auto">
        {workspaces.length === 0 && (
          <p className="mt-3 text-[13px] text-muted-foreground">
            Nessun workspace. Aggiungi una cartella per iniziare.
          </p>
        )}

        {workspaces.map((ws) => (
          <div
            key={ws.id}
            onClick={(e) => {
              // I click su pulsanti, campi e menu (anche quelli nel portale,
              // che risalgono lungo l'albero React) non selezionano il gruppo.
              if ((e.target as HTMLElement).closest("button, input, a, [role^='menu']")) {
                return;
              }
              onSelect(ws.id);
            }}
            className={cn(
              "rounded-lg border bg-card p-2.5",
              activeWorkspaceId === ws.id && "border-primary/40 bg-accent/40",
            )}
          >
            {editingId === ws.id ? (
              <div className="flex flex-col gap-2">
                <Input
                  value={editingName}
                  onChange={(e) => setEditingName(e.target.value)}
                  placeholder="Nome workspace"
                  autoFocus
                  className="h-8 text-[13px]"
                />
                <div className="flex flex-row gap-1.5">
                  <Button
                    size="sm"
                    disabled={busyId === ws.id}
                    onClick={() => void saveEdit(ws.id)}
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
              <div className="flex flex-col gap-1.5">
                <div className="flex items-center gap-1.5">
                  <span className="min-w-0 flex-1 truncate text-sm font-semibold">
                    {ws.name}
                  </span>
                  <Badge variant={ws.kind === "wsl" ? "default" : "secondary"}>
                    {ws.kind === "wsl" ? (ws.distro ?? "WSL") : "WIN"}
                  </Badge>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="h-7 w-7"
                    title="Apri cartella"
                    aria-label="Apri cartella"
                    onClick={() => openFolder(ws.id)}
                  >
                    <FolderOpen />
                  </Button>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        size="icon"
                        variant="ghost"
                        className="h-7 w-7"
                        title="Altre azioni"
                        aria-label="Altre azioni"
                      >
                        <Ellipsis />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent
                      align="end"
                      // Niente ritorno del focus al trigger: la rinomina inline
                      // deve poter prendere il focus sull'input.
                      onCloseAutoFocus={(e) => e.preventDefault()}
                    >
                      {ws.launches.length === 0 ? (
                        <DropdownMenuItem disabled>Nessun avvio</DropdownMenuItem>
                      ) : (
                        ws.launches.map((l) => (
                          <DropdownMenuItem
                            key={l.id}
                            title={l.command}
                            onSelect={() => onSpawn(ws, l)}
                          >
                            <span className="min-w-0 flex-1 truncate">{l.name}</span>
                            {l.is_default && (
                              <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
                                <Star className="!size-3 fill-current" />
                                predefinito
                              </span>
                            )}
                          </DropdownMenuItem>
                        ))
                      )}
                      <DropdownMenuSeparator />
                      <DropdownMenuItem onSelect={() => onManageLaunches(ws)}>
                        Gestisci avvii…
                      </DropdownMenuItem>
                      <DropdownMenuItem onSelect={() => startEdit(ws)}>
                        Rinomina
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        className="text-destructive focus:text-destructive"
                        onSelect={() => {
                          setError(null);
                          setConfirmingId(ws.id);
                        }}
                      >
                        Elimina
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>

                <p className="truncate text-xs text-muted-foreground">{ws.path}</p>

                {confirmingId === ws.id ? (
                  <div className="flex items-center gap-1.5">
                    <span className="flex-1 text-xs">Rimuovere?</span>
                    <Button
                      size="sm"
                      variant="destructive"
                      disabled={busyId === ws.id}
                      onClick={() => void confirmRemove(ws.id)}
                    >
                      Sì
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => setConfirmingId(null)}
                    >
                      No
                    </Button>
                  </div>
                ) : (
                  <div className="flex flex-row gap-1.5">
                    <Button
                      size="sm"
                      title={
                        defaultLaunch(ws)
                          ? `Esegue: ${defaultLaunch(ws)!.name}`
                          : "Apre una shell vuota"
                      }
                      onClick={() => onSpawn(ws)}
                    >
                      + Terminale
                    </Button>
                  </div>
                )}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
