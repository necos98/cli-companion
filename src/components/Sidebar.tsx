import { useEffect, useState } from "react";
import {
  ChevronDown,
  Ellipsis,
  FolderOpen,
  ListChecks,
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
import EnvChip from "./EnvChip";
import StatusIcon from "./StatusIcon";
import { api } from "../lib/tauri";
import BotAvatar from "./BotAvatar";
import { cn } from "../lib/utils";
import type { BuiltinLaunch, Launch, Workspace } from "../lib/tauri";
import type { TaskCounts } from "../lib/board";
import { resolveDefaultLaunch } from "../lib/launch";
import { GROUP_STATUS } from "../lib/agents";
import type { TabStatus } from "../lib/agents";
import { groupIndicator } from "../lib/layout";
import { dropOrder, dropPositionAt } from "../lib/workspaceOrder";
import type { DropPosition } from "../lib/workspaceOrder";

/** Terminale aperto di un workspace, come mostrato nella sua card. */
export interface SidebarTerminal {
  key: string;
  label: string;
  /** Nome del bot della sessione; assente = funzione spenta. */
  bot?: string;
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
  /** Conteggi dei task (in corso / in coda) di ogni workspace, dal suo `BoardView`. */
  taskCounts: Record<string, Pick<TaskCounts, "running" | "queued">>;
  /** Workspace di cui è mostrato il Tasks Manager al posto dei terminali (se c'è). */
  managerShownId: string | null;
  /** Mostra/nasconde il Tasks Manager del workspace. */
  onToggleManager: (id: string) => void;
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
  /** Archivia il workspace (chiude i suoi terminali e lo nasconde). */
  onArchive: (id: string) => Promise<void>;
  /** Nuovo ordine dei workspace (trascinamento). */
  onReorder: (ids: string[]) => Promise<void>;
  onAdd: () => void;
  /** Avviso dell'app (es. avvio rifiutato), mostrato come gli errori della lista. */
  notice?: string | null;
}

const ICON_BTN =
  "flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[5px] text-muted-foreground transition-colors hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring [&_svg]:size-[13px]";

const MENU_ITEM =
  "rounded-md px-2.5 py-[7px] text-[13px] text-text-secondary focus:bg-surface-2 focus:text-foreground";

const MENU_CONTENT = "w-[220px] rounded-[10px] border-border-strong bg-surface p-1";

/** Contatori del pulsante Tasks: in corso (ambra) · in coda (attenuato); niente se entrambi a zero. */
function TaskCountsLabel({ counts }: { counts: Pick<TaskCounts, "running" | "queued"> | undefined }) {
  const running = counts?.running ?? 0;
  const queued = counts?.queued ?? 0;
  if (running === 0 && queued === 0) return null;
  return (
    <span
      className="ml-auto flex items-center gap-1 font-mono text-[11px]"
      title={`${running} in corso · ${queued} in coda`}
    >
      <span className={running > 0 ? "text-status-running" : "text-faint"}>{running}</span>
      <span className="text-faint" aria-hidden="true">
        ·
      </span>
      <span className="text-muted-foreground">{queued}</span>
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
  taskCounts,
  managerShownId,
  onToggleManager,
  onSelect,
  onFocusTerminal,
  builtinLaunches,
  globalLaunch,
  onSpawn,
  onManageLaunches,
  onRename,
  onRemove,
  onArchive,
  onReorder,
  onAdd,
  notice = null,
}: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [archivingId, setArchivingId] = useState<string | null>(null);
  // Trascinamento per l'ordine: riga trascinata e posizione di rilascio.
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<{ id: string; pos: DropPosition } | null>(null);
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
    setArchivingId(null);
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

  const confirmArchive = async (id: string) => {
    setBusyId(id);
    setError(null);
    try {
      await onArchive(id);
      setArchivingId(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  };

  const endDrag = () => {
    setDragId(null);
    setDropTarget(null);
  };

  const handleDrop = () => {
    if (dragId && dropTarget) {
      const ids = workspaces.map((w) => w.id);
      const next = dropOrder(ids, dragId, dropTarget.id, dropTarget.pos);
      if (next !== ids) {
        setError(null);
        onReorder(next).catch((e) => setError(String(e)));
      }
    }
    endDrag();
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
          onSelect={() => {
            setError(null);
            setConfirmingId(null);
            setArchivingId(ws.id);
          }}
          className={MENU_ITEM}
        >
          Archivia
        </DropdownMenuItem>
        <DropdownMenuItem
          className={cn(MENU_ITEM, "text-destructive focus:text-destructive")}
          onSelect={() => {
            setError(null);
            setArchivingId(null);
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
        draggable
        onDragStart={(e) => {
          // Niente trascinamento partendo da campi di testo.
          if ((e.target as HTMLElement).closest("input")) return e.preventDefault();
          e.dataTransfer.effectAllowed = "move";
          e.dataTransfer.setData("text/plain", ws.id);
          setDragId(ws.id);
        }}
        onDragEnd={endDrag}
        onDragOver={(e) => {
          if (!dragId) return;
          e.preventDefault();
          e.dataTransfer.dropEffect = "move";
          const r = e.currentTarget.getBoundingClientRect();
          const pos = dropPositionAt(e.clientY, r.top, r.height);
          setDropTarget((d) => (d && d.id === ws.id && d.pos === pos ? d : { id: ws.id, pos }));
        }}
        onDrop={(e) => {
          if (!dragId) return;
          e.preventDefault();
          handleDrop();
        }}
        onClick={(e) => {
          // I click su pulsanti, campi e menu (anche quelli nel portale, che
          // risalgono lungo l'albero React) non selezionano il gruppo.
          if ((e.target as HTMLElement).closest("button, input, a, [role^='menu']")) return;
          setExpandedId(ws.id);
          onSelect(ws.id);
        }}
        className={cn(
          "group relative cursor-pointer rounded-[10px] border transition-colors",
          dragId === ws.id && "opacity-50",
          expanded
            ? "border-border bg-surface pb-1.5 pt-1"
            : "border-transparent hover:border-border hover:bg-surface/60",
        )}
      >
        {dragId && dragId !== ws.id && dropTarget?.id === ws.id && (
          <span
            aria-hidden
            className={cn(
              "pointer-events-none absolute left-1 right-1 h-0.5 rounded bg-primary",
              dropTarget.pos === "before" ? "-top-[2px]" : "-bottom-[2px]",
            )}
          />
        )}
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
                {(taskCounts[ws.id]?.running ?? 0) > 0 && (
                  <span
                    className="rounded bg-status-running/15 px-1 font-mono text-[10px] text-status-running"
                    title={`Tasks: ${taskCounts[ws.id].running} in corso`}
                  >
                    {taskCounts[ws.id].running}
                  </span>
                )}
                {list.length > 0 && (
                  <span className="font-mono text-[11px] text-faint group-focus-within:hidden group-hover:hidden group-has-[[data-state=open]]:hidden">
                    {list.length}
                  </span>
                )}
                {/* Azioni visibili solo con il mouse sopra (o il focus dentro). Con un
                    menu aperto restano visibili: il menu è non modale, il focus passa
                    al portale e il mouse esce dalla riga, ma se il trigger sparisse
                    (display: none) l'ancora avrebbe un rettangolo nullo e Radix
                    riposizionerebbe il menu in cima alla finestra. */}
                <span className="hidden items-center gap-0.5 group-focus-within:flex group-hover:flex has-[[data-state=open]]:flex">
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

        {archivingId === ws.id && (
          <div className="flex flex-col gap-1.5 px-2.5 pb-1.5 pt-1">
            <span className="text-xs">
              Archiviare {ws.name}?
              {list.length > 0 &&
                ` Verranno chiusi i suoi ${list.length} terminali aperti e`}
              {list.length > 0 ? " la" : " La"} coda resterà ferma finché non lo ripristini.
            </span>
            <div className="flex items-center gap-1.5">
              <Button
                size="sm"
                disabled={busyId === ws.id}
                onClick={() => void confirmArchive(ws.id)}
              >
                Sì
              </Button>
              <Button size="sm" variant="secondary" onClick={() => setArchivingId(null)}>
                No
              </Button>
            </div>
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
                      title={t.bot ? `${t.bot} · ${t.label}` : t.label}
                      className={cn(
                        "flex w-full items-center gap-2 rounded-md py-1.5 pl-3 pr-2 text-left transition-colors hover:bg-surface-2",
                        t.active ? "bg-surface-2 text-foreground" : "text-text-secondary",
                      )}
                    >
                      <span className="flex w-3.5 flex-shrink-0 justify-center">
                        <StatusIcon status={t.status} />
                      </span>
                      {t.bot && <BotAvatar name={t.bot} size={16} />}
                      <span className="min-w-0 flex-1 truncate font-mono text-xs">
                        {t.bot ? `${t.bot} · ${t.label}` : t.label}
                      </span>
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

            {/* Tasks Manager del workspace (Ctrl+Maiusc+B): in corso · in coda. */}
            <div className="mx-1.5 mt-1.5">
              <button
                type="button"
                aria-pressed={managerShownId === ws.id}
                title={
                  managerShownId === ws.id
                    ? "Torna ai terminali (Ctrl+Maiusc+B)"
                    : "Apri il Tasks Manager (Ctrl+Maiusc+B)"
                }
                onClick={() => onToggleManager(ws.id)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md border px-2.5 py-1.5 transition-colors",
                  managerShownId === ws.id
                    ? "border-primary/50 bg-surface-2 text-foreground"
                    : "border-border-strong text-muted-foreground hover:border-muted-foreground hover:text-foreground",
                )}
              >
                <ListChecks className="h-3 w-3 flex-shrink-0" />
                <span className="text-xs">Tasks</span>
                <TaskCountsLabel counts={taskCounts[ws.id]} />
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

      {(error ?? notice) && (
        <p className="px-4 pb-2 text-xs text-destructive">{error ?? notice}</p>
      )}

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
