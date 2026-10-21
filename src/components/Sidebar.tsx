import { useEffect, useState } from "react";
import {
  ChevronDown,
  Ellipsis,
  FolderOpen,
  LayoutDashboard,
  Moon,
  Pin,
  Plus,
  Star,
} from "lucide-react";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { Input } from "./ui/input";
import StatusIcon from "./StatusIcon";
import { api } from "../lib/tauri";
import { cn } from "../lib/utils";
import type { BuiltinLaunch, Launch, Workspace } from "../lib/tauri";
import { resolveDefaultLaunch } from "../lib/launch";
import { GROUP_STATUS } from "../lib/agents";
import type { TabStatus } from "../lib/agents";
import { groupIndicator } from "../lib/layout";

/** Terminale aperto di un workspace, come mostrato nella sua card. */
export interface SidebarTerminal {
  key: string;
  label: string;
  status: TabStatus;
  pinned: boolean;
  /** È il pannello attivo del gruppo mostrato. */
  active: boolean;
}

interface Props {
  workspaces: Workspace[];
  /** Workspace del gruppo di terminali attivo (null con la vista globale). */
  activeWorkspaceId: string | null;
  /** Terminali aperti per id di workspace (ordine della griglia). */
  terminals: Record<string, SidebarTerminal[]>;
  /** Task attivi (in corso + da integrare) della board di ogni workspace. */
  boardActive: Record<string, number>;
  /** Workspace con l'orchestratore automatico attivo: testo di stato per il tooltip. */
  autoStatus: Record<string, string>;
  /** Workspace di cui è mostrata la board al posto dei terminali (se c'è). */
  boardShownId: string | null;
  /** Mostra/nasconde la board del workspace. */
  onToggleBoard: (id: string) => void;
  /** Click sulla riga (non sui suoi pulsanti): seleziona il gruppo, se ha terminali. */
  onSelect: (id: string) => void;
  /** Click su un terminale della lista: lo porta in primo piano. */
  onFocusTerminal: (key: string) => void;
  /** Apre un terminale; senza `launch` esegue l'avvio predefinito (se c'è). */
  onSpawn: (ws: Workspace, launch?: Launch | BuiltinLaunch) => void;
  /** Avvii integrati (in cima al menu di ogni workspace). */
  builtinLaunches: BuiltinLaunch[];
  /** Avvio predefinito globale (id di un integrato; null = shell vuota). */
  globalLaunch: string | null;
  /** Apre le impostazioni sugli avvii del workspace. */
  onManageLaunches: (ws: Workspace) => void;
  onRename: (id: string, name: string) => Promise<void>;
  onRemove: (id: string) => Promise<void>;
  onAdd: () => void;
}

const ICON_BTN =
  "flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[5px] text-muted-foreground transition-colors hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring [&_svg]:size-[13px]";

const MENU_ITEM =
  "rounded-md px-2.5 py-[7px] text-[13px] text-text-secondary focus:bg-surface-2 focus:text-foreground";

const MENU_CONTENT = "w-[220px] rounded-[10px] border-border-strong bg-surface p-1";

function EnvChip({ ws }: { ws: Workspace }) {
  const wsl = ws.kind === "wsl";
  return (
    <span
      className={cn(
        "flex-shrink-0 rounded border px-1 py-px font-mono text-[10px] font-medium",
        wsl ? "border-primary/40 text-primary" : "border-border-strong text-muted-foreground",
      )}
    >
      {wsl ? "WSL" : "WIN"}
    </span>
  );
}

function wsPath(ws: Workspace): string {
  return ws.kind === "wsl" && ws.distro ? `${ws.distro} · ${ws.path}` : ws.path;
}

/** Voci dei menu di avvio (⋯ e caret del pulsante diviso). */
function LaunchItems({
  ws,
  builtins,
  globalLaunch,
  onSpawn,
}: {
  ws: Workspace;
  builtins: BuiltinLaunch[];
  globalLaunch: string | null;
  onSpawn: (ws: Workspace, launch?: Launch | BuiltinLaunch) => void;
}) {
  const defId = resolveDefaultLaunch(ws, builtins, globalLaunch)?.id;
  const item = (l: Launch | BuiltinLaunch) => (
    <DropdownMenuItem
      key={l.id}
      title={l.command}
      onSelect={() => onSpawn(ws, l)}
      className={MENU_ITEM}
    >
      <span className="min-w-0 flex-1 truncate font-mono text-xs">{l.name}</span>
      {l.id === defId && (
        <span className="flex items-center gap-1 text-[10px] text-primary">
          <Star className="!size-3 fill-current" />
          predefinito
        </span>
      )}
    </DropdownMenuItem>
  );
  return (
    <>
      <DropdownMenuLabel className="px-2.5 pb-1 pt-1.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-faint">
        Avvii
      </DropdownMenuLabel>
      {builtins.map(item)}
      {builtins.length > 0 && ws.launches.length > 0 && (
        <DropdownMenuSeparator className="mx-1.5 bg-border" />
      )}
      {ws.launches.map(item)}
      {builtins.length === 0 && ws.launches.length === 0 && (
        <DropdownMenuItem disabled className={MENU_ITEM}>
          Nessun avvio
        </DropdownMenuItem>
      )}
    </>
  );
}

export default function Sidebar({
  workspaces,
  activeWorkspaceId,
  terminals,
  boardActive,
  autoStatus,
  boardShownId,
  onToggleBoard,
  onSelect,
  onFocusTerminal,
  builtinLaunches,
  globalLaunch,
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
  // Workspace espanso a card: segue il gruppo attivo, ma si può selezionare
  // anche un workspace senza terminali (che non ha un gruppo).
  const [expandedId, setExpandedId] = useState<string | null>(null);

  useEffect(() => {
    if (activeWorkspaceId) setExpandedId(activeWorkspaceId);
  }, [activeWorkspaceId]);

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

  // Il dialog delle impostazioni si apre solo a menu chiuso, così i due layer
  // Radix non si sovrappongono (altrimenti resta `pointer-events: none`).
  const manage = (ws: Workspace) => setTimeout(() => onManageLaunches(ws), 0);

  /** Menu ⋯ (non modale: un menu modale che apre un Dialog dal suo onSelect
      lascia `pointer-events: none` bloccato sul body). */
  const renderMenu = (ws: Workspace) => (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger asChild>
        <button type="button" className={ICON_BTN} title="Altre azioni" aria-label="Altre azioni">
          <Ellipsis />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className={MENU_CONTENT}
        // Niente ritorno del focus al trigger: la rinomina inline deve poter
        // prendere il focus sull'input.
        onCloseAutoFocus={(e) => e.preventDefault()}
      >
        <LaunchItems
          ws={ws}
          builtins={builtinLaunches}
          globalLaunch={globalLaunch}
          onSpawn={onSpawn}
        />
        <DropdownMenuSeparator className="mx-1.5 bg-border" />
        <DropdownMenuItem onSelect={() => manage(ws)} className={MENU_ITEM}>
          Gestisci avvii…
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => startEdit(ws)} className={MENU_ITEM}>
          Rinomina
        </DropdownMenuItem>
        <DropdownMenuItem
          className={cn(MENU_ITEM, "text-destructive focus:text-destructive")}
          onSelect={() => {
            setError(null);
            setConfirmingId(ws.id);
          }}
        >
          Elimina
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );

  const renderRow = (ws: Workspace) => {
    const list = terminals[ws.id] ?? [];
    const expanded = expandedId === ws.id;
    const def = resolveDefaultLaunch(ws, builtinLaunches, globalLaunch);
    const indicator = groupIndicator(list.map((t) => t.status));

    if (editingId === ws.id) {
      return (
        <div key={ws.id} className="rounded-[10px] border border-border bg-surface">
          <div className="flex flex-col gap-2 px-2.5 py-2">
            <Input
              value={editingName}
              onChange={(e) => setEditingName(e.target.value)}
              placeholder="Nome workspace"
              autoFocus
              className="h-8 text-[13px]"
            />
            <div className="flex flex-row gap-1.5">
              <Button size="sm" disabled={busyId === ws.id} onClick={() => void saveEdit(ws.id)}>
                Salva
              </Button>
              <Button size="sm" variant="secondary" onClick={() => setEditingId(null)}>
                Annulla
              </Button>
            </div>
          </div>
        </div>
      );
    }

    return (
      <div
        key={ws.id}
        role="group"
        aria-label={ws.name}
        onClick={(e) => {
          // I click su pulsanti, campi e menu (anche quelli nel portale, che
          // risalgono lungo l'albero React) non selezionano il gruppo.
          if ((e.target as HTMLElement).closest("button, input, a, [role^='menu']")) return;
          setExpandedId(ws.id);
          onSelect(ws.id);
        }}
        className={cn(
          "group cursor-pointer rounded-[10px] border transition-colors",
          expanded
            ? "border-border bg-surface pb-1.5 pt-1"
            : "border-transparent hover:border-border hover:bg-surface/60",
        )}
      >
        <div
          className={cn("flex flex-col gap-0.5 px-2.5", expanded ? "pb-1 pt-1.5" : "py-2")}
        >
          <div className="flex min-h-6 items-center gap-2">
            <EnvChip ws={ws} />
            <span
              className={cn(
                "min-w-0 flex-1 truncate text-[13px]",
                expanded ? "font-semibold text-foreground" : "font-medium text-text-secondary",
              )}
            >
              {ws.name}
            </span>
            {autoStatus[ws.id] !== undefined && (
              <span title={autoStatus[ws.id]} role="img" aria-label={autoStatus[ws.id]} className="flex flex-shrink-0 text-primary">
                <Moon className="h-3 w-3" />
              </span>
            )}
            {expanded ? (
              <>
                <button
                  type="button"
                  className={ICON_BTN}
                  title="Apri cartella"
                  aria-label="Apri cartella"
                  onClick={() => openFolder(ws.id)}
                >
                  <FolderOpen />
                </button>
                {renderMenu(ws)}
              </>
            ) : (
              <>
                {indicator !== "none" && <StatusIcon status={GROUP_STATUS[indicator]} />}
                {(boardActive[ws.id] ?? 0) > 0 && (
                  <span
                    className="rounded bg-status-running/15 px-1 font-mono text-[10px] text-status-running"
                    title={`Board: ${boardActive[ws.id]} task attivi`}
                  >
                    {boardActive[ws.id]}
                  </span>
                )}
                {list.length > 0 && (
                  <span className="font-mono text-[11px] text-faint group-focus-within:hidden group-hover:hidden">
                    {list.length}
                  </span>
                )}
                {/* Azioni visibili solo con il mouse sopra (o il focus dentro). */}
                <span className="hidden items-center gap-0.5 group-focus-within:flex group-hover:flex">
                  <button
                    type="button"
                    className={ICON_BTN}
                    title={def ? `Nuovo terminale: ${def.name}` : "Nuovo terminale"}
                    aria-label="Nuovo terminale"
                    onClick={() => onSpawn(ws)}
                  >
                    <Plus />
                  </button>
                  <button
                    type="button"
                    className={ICON_BTN}
                    title="Apri cartella"
                    aria-label="Apri cartella"
                    onClick={() => openFolder(ws.id)}
                  >
                    <FolderOpen />
                  </button>
                  {renderMenu(ws)}
                </span>
              </>
            )}
          </div>
          <p
            className="truncate pl-9 font-mono text-[11px] text-muted-foreground"
            title={wsPath(ws)}
          >
            {wsPath(ws)}
          </p>
        </div>

        {confirmingId === ws.id && (
          <div className="flex items-center gap-1.5 px-2.5 pb-1.5 pt-1">
            <span className="flex-1 text-xs">Rimuovere?</span>
            <Button
              size="sm"
              variant="destructive"
              disabled={busyId === ws.id}
              onClick={() => void confirmRemove(ws.id)}
            >
              Sì
            </Button>
            <Button size="sm" variant="secondary" onClick={() => setConfirmingId(null)}>
              No
            </Button>
          </div>
        )}

        {expanded && (
          <>
            {list.length > 0 && (
              <ul className="mt-0.5 flex flex-col">
                {list.map((t) => (
                  <li key={t.key} className="mx-1.5">
                    <button
                      type="button"
                      onClick={() => onFocusTerminal(t.key)}
                      title={t.label}
                      className={cn(
                        "flex w-full items-center gap-2 rounded-md py-1.5 pl-3 pr-2 text-left transition-colors hover:bg-surface-2",
                        t.active ? "bg-surface-2 text-foreground" : "text-text-secondary",
                      )}
                    >
                      <span className="flex w-3.5 flex-shrink-0 justify-center">
                        <StatusIcon status={t.status} />
                      </span>
                      <span className="min-w-0 flex-1 truncate font-mono text-xs">{t.label}</span>
                      {t.pinned && (
                        <Pin
                          role="img"
                          aria-label="Fissato nella vista globale"
                          className="h-3 w-3 flex-shrink-0 text-primary"
                        />
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            )}

            {/* Board dei task del workspace (Ctrl+Maiusc+B). */}
            <div className="mx-1.5 mt-1.5">
              <button
                type="button"
                aria-pressed={boardShownId === ws.id}
                title={boardShownId === ws.id ? "Torna ai terminali (Ctrl+Maiusc+B)" : "Mostra la board (Ctrl+Maiusc+B)"}
                onClick={() => onToggleBoard(ws.id)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md border px-2.5 py-1.5 transition-colors",
                  boardShownId === ws.id
                    ? "border-primary/50 bg-surface-2 text-foreground"
                    : "border-border-strong text-muted-foreground hover:border-muted-foreground hover:text-foreground",
                )}
              >
                <LayoutDashboard className="h-3 w-3 flex-shrink-0" />
                <span className="text-xs">Board</span>
                {(boardActive[ws.id] ?? 0) > 0 && (
                  <span className="ml-auto rounded bg-status-running/15 px-1.5 font-mono text-[11px] text-status-running">
                    {boardActive[ws.id]}
                  </span>
                )}
              </button>
            </div>

            {/* Pulsante diviso: avvio predefinito + scelta dell'avvio. */}
            <div className="mx-1.5 mt-1.5 flex gap-1">
              <button
                type="button"
                title={def ? `Esegue: ${def.name}` : "Apre una shell vuota"}
                onClick={() => onSpawn(ws)}
                className="flex min-w-0 flex-1 items-center gap-2 rounded-md border border-dashed border-border-strong px-2.5 py-1.5 text-muted-foreground transition-colors hover:border-muted-foreground hover:text-foreground"
              >
                <Plus className="h-3 w-3 flex-shrink-0" />
                <span className="text-xs">Terminale</span>
                {def && (
                  <span className="ml-auto min-w-0 truncate font-mono text-[11px] text-muted-foreground">
                    {def.name}
                  </span>
                )}
              </button>
              <DropdownMenu modal={false}>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    aria-label="Scegli un avvio"
                    title="Scegli un avvio"
                    className="flex w-[30px] flex-shrink-0 items-center justify-center rounded-md border border-dashed border-border-strong text-muted-foreground transition-colors hover:border-muted-foreground hover:text-foreground"
                  >
                    <ChevronDown className="h-3 w-3" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent
                  align="end"
                  className={MENU_CONTENT}
                  onCloseAutoFocus={(e) => e.preventDefault()}
                >
                  <LaunchItems
          ws={ws}
          builtins={builtinLaunches}
          globalLaunch={globalLaunch}
          onSpawn={onSpawn}
        />
                  <DropdownMenuSeparator className="mx-1.5 bg-border" />
                  <DropdownMenuItem onSelect={() => manage(ws)} className={MENU_ITEM}>
                    Gestisci avvii…
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </>
        )}
      </div>
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex items-center justify-between pb-2 pl-4 pr-3 pt-3.5">
        <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-faint">
          Workspace
        </span>
        <button
          type="button"
          onClick={onAdd}
          title="Aggiungi workspace"
          aria-label="Aggiungi workspace"
          className="flex h-7 w-7 items-center justify-center rounded-[7px] border border-border-strong text-foreground transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <Plus className="h-3.5 w-3.5" />
        </button>
      </div>

      {error && <p className="px-4 pb-2 text-xs text-destructive">{error}</p>}

      <div className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2">
        {workspaces.length === 0 && (
          <p className="mt-3 px-2 text-[13px] text-muted-foreground">
            Nessun workspace. Aggiungi una cartella per iniziare.
          </p>
        )}
        {workspaces.map(renderRow)}
      </div>
    </div>
  );
}
