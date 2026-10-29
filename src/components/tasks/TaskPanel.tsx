import { useCallback, useMemo, useState } from "react";
import {
  AlertTriangle,
  ExternalLink,
  ListOrdered,
  Loader2,
  Pause,
  Play,
  Plus,
  Sparkles,
  XCircle,
  ChevronsLeft,
} from "lucide-react";
import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import { failedTasks, isParallel, queuedTasks, runningTasks, taskCounts } from "../../lib/board";
import type { BoardView } from "../../lib/board";
import PanelFailedCard from "./PanelFailedCard";
import PanelHeaderButton from "./PanelHeaderButton";
import PanelQueue from "./PanelQueue";
import PanelRail from "./PanelRail";
import type { PanelRailItem } from "./PanelRail";
import PanelRunningCard from "./PanelRunningCard";
import PanelSection from "./PanelSection";
import ProposalCard from "./ProposalCard";
import SettingsProposalCard from "./SettingsProposalCard";
import { usePanelNow } from "./usePanelNow";

/**
 * Props del pannello Task di un workspace (accanto alla griglia dei terminali).
 *
 * Il pannello chiama direttamente `api` (src/lib/tauri.ts) per le azioni
 * (accetta/scarta proposte, riprova, pausa, riordina…) e passa a `onBoard` la
 * `BoardView` restituita: App la fonde nello stato (`mergeBoard` per `rev`).
 * Gli eventi `board-update` arrivano comunque da soli.
 */
export interface TaskPanelProps {
  workspace: Workspace;
  /** Coda corrente; null finché `board_get` non ha risposto (o è fallito). */
  board: BoardView | null;
  /** Errore del caricamento iniziale della board, se c'è. */
  loadError?: string | null;
  /** Compresso (barra di 48 px con i soli contatori) o espanso (~284 px). Ricordato da App per workspace. */
  collapsed: boolean;
  onToggleCollapsed: () => void;
  /** Aggiorna la board in App con la risposta di un comando. */
  onBoard: (board: BoardView) => void;
  /** Aggiorna il workspace in App (proposta di impostazioni applicata). */
  onWorkspaceUpdated: (workspace: Workspace) => void;
  /** Apre il Tasks Manager del workspace, col dettaglio di `taskId` se dato. */
  onOpenManager: (taskId?: string) => void;
  /** Apre un nuovo tab Task Intake nel workspace. */
  onOpenIntake: () => void;
  /** Apre il Tasks Manager con il dialog "Nuovo task". */
  onNewTask: () => void;
  /** Mette a fuoco un tab nella griglia dei terminali (es. il worker di un task in corso). */
  onFocusTab: (tabId: string) => void;
  /** Nome del bot di un tab (se la funzione e' attiva). */
  botOf?: (tabId: string) => string | undefined;
}

const FOOTER_BUTTON =
  "flex h-8 flex-1 items-center justify-center gap-1.5 rounded-md border border-border-strong text-[13px] transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

/** Pannello Task del workspace: proposte, in corso, in coda trascinabile, falliti. */
export default function TaskPanel({
  workspace,
  board,
  loadError,
  collapsed,
  onToggleCollapsed,
  onBoard,
  onWorkspaceUpdated,
  onOpenManager,
  onOpenIntake,
  onNewTask,
  onFocusTab,
  botOf,
}: TaskPanelProps) {
  const workspaceId = workspace.id;
  const [error, setError] = useState<string | null>(null);
  const [pausing, setPausing] = useState(false);

  const counts = useMemo(() => taskCounts(board), [board]);
  const running = useMemo(() => (board ? runningTasks(board) : []), [board]);
  const queued = useMemo(() => (board ? queuedTasks(board) : []), [board]);
  const failed = useMemo(() => (board ? failedTasks(board) : []), [board]);
  const now = usePanelNow(!collapsed && running.length > 0);

  /** Esegue un comando sulla board: la risposta va ad App, un errore resta in linea. */
  const run = useCallback(
    async (command: () => Promise<BoardView>) => {
      setError(null);
      try {
        onBoard(await command());
      } catch (e) {
        setError(String(e));
      }
    },
    [onBoard],
  );

  const togglePause = async () => {
    if (!board) return;
    setPausing(true);
    await run(() => api.boardSet(workspaceId, { paused: !board.paused }));
    setPausing(false);
  };
  const retry = useCallback(
    (taskId: string) => run(() => api.taskAction(workspaceId, taskId, "retry")),
    [run, workspaceId],
  );
  const move = useCallback(
    (taskId: string, beforeId: string | null) => {
      void run(() => api.taskMove(workspaceId, taskId, beforeId ?? undefined));
    },
    [run, workspaceId],
  );

  if (collapsed) {
    const items: PanelRailItem[] = [
      { key: "proposals", icon: Sparkles, count: counts.proposals, label: "proposte in attesa", className: "text-primary" },
      {
        key: "running",
        icon: Loader2,
        count: counts.running - counts.attention,
        label: "task in corso",
        className: "text-status-running",
      },
      {
        key: "attention",
        icon: AlertTriangle,
        count: counts.attention,
        label: "task richiedono attenzione",
        className: "text-status-attention",
      },
      { key: "queued", icon: ListOrdered, count: counts.queued, label: "task in coda", className: "text-muted-foreground" },
      { key: "failed", icon: XCircle, count: counts.failed, label: "task falliti", className: "text-destructive" },
    ];
    return (
      <PanelRail
        ariaLabel="Task del workspace, compresso"
        items={items.filter((i) => i.count > 0 || i.key === "queued" || i.key === "running")}
        onExpand={onToggleCollapsed}
      />
    );
  }

  const parallel = board ? isParallel(board.delivery ?? { mode: "commit", parallel: false }) : false;
  const hasActive = running.length + queued.length + failed.length + counts.proposals > 0;

  return (
    <aside
      aria-label="Task del workspace"
      className="flex min-h-0 w-[284px] flex-shrink-0 flex-col overflow-hidden rounded-[10px] border bg-base"
    >
      <header className="flex h-10 flex-shrink-0 items-center gap-1.5 border-b bg-pane-header pl-3 pr-2">
        <span className="flex-1 text-[13px] font-semibold">Task</span>
        {board && (
          <PanelHeaderButton
            icon={board.paused ? Play : Pause}
            label={board.paused ? "Riprendi la coda" : "Metti in pausa la coda"}
            onClick={() => void togglePause()}
            disabled={pausing}
            iconClassName={board.paused ? "fill-status-running text-status-running" : undefined}
          />
        )}
        <PanelHeaderButton
          icon={ExternalLink}
          label="Apri Tasks Manager"
          title="Apri Tasks Manager (Ctrl+Maiusc+B)"
          onClick={() => onOpenManager()}
        />
        <PanelHeaderButton icon={ChevronsLeft} label="Comprimi il pannello Task" onClick={onToggleCollapsed} />
      </header>

      {board?.paused && (
        <div className="mx-2.5 mt-2.5 rounded-lg bg-status-running/10 px-2.5 py-2 text-xs text-status-running">
          Coda in pausa: i task in corso finiscono, nessuno nuovo parte.
        </div>
      )}
      {board?.schedule_blocked && (
        <div className="mx-2.5 mt-2.5 rounded-lg bg-status-running/10 px-2.5 py-2 text-xs text-status-running">
          Fuori fascia oraria: nessun task parte finché la fascia non si apre.
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto overflow-x-hidden p-2.5">
        {!board ? (
          <p className="px-1 text-xs text-muted-foreground" role={loadError ? "alert" : "status"}>
            {loadError ? <span className="break-words text-destructive">{loadError}</span> : "Caricamento…"}
          </p>
        ) : (
          <>
            {board.proposals.map((p) => (
              <ProposalCard key={p.id} workspaceId={workspaceId} proposal={p} board={board} onBoard={onBoard} />
            ))}
            {board.settings_proposals.map((p) => (
              <SettingsProposalCard
                key={p.id}
                workspaceId={workspaceId}
                proposal={p}
                onBoard={onBoard}
                onWorkspace={onWorkspaceUpdated}
              />
            ))}

            {running.length > 0 && (
              <PanelSection
                title="IN CORSO"
                count={parallel ? `${running.length}/${board.max_parallel}` : running.length}
              >
                {running.map((t) => (
                  <PanelRunningCard
                    key={t.id}
                    task={t}
                    now={now}
                    bot={t.session ? botOf?.(t.session) : undefined}
                    onClick={() => (t.session ? onFocusTab(t.session) : onOpenManager(t.id))}
                  />
                ))}
              </PanelSection>
            )}

            {queued.length > 0 && (
              <PanelSection title="IN CODA" count={queued.length}>
                <PanelQueue queue={queued} allTasks={board.tasks} onOpen={onOpenManager} onMove={move} />
              </PanelSection>
            )}

            {failed.length > 0 && (
              <PanelSection title="FALLITI" titleClassName="text-destructive">
                {failed.map((t) => (
                  <PanelFailedCard key={t.id} task={t} onRetry={retry} onOpen={onOpenManager} />
                ))}
              </PanelSection>
            )}

            {!hasActive && (
              <div className="flex flex-col items-center gap-2 px-2 py-6 text-center">
                <p className="text-[13px] font-medium">{counts.total === 0 ? "Nessun task" : "Nessun task attivo"}</p>
                {counts.total > 0 && (
                  <p className="text-xs text-muted-foreground">
                    {counts.done === 1 ? "1 task fatto" : `${counts.done} task fatti`}
                  </p>
                )}
                {counts.total === 0 && (
                  <button
                    type="button"
                    onClick={onOpenIntake}
                    className="mt-1 flex h-8 items-center gap-1.5 rounded-md border border-border-strong bg-surface px-3 text-[13px] transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <Sparkles className="h-3.5 w-3.5 text-primary" aria-hidden="true" />
                    Apri Task Intake
                  </button>
                )}
              </div>
            )}
          </>
        )}
      </div>

      {error && (
        <p role="alert" className="flex-shrink-0 break-words border-t px-2.5 py-2 text-[11px] text-destructive">
          {error}
        </p>
      )}

      <footer className="flex flex-shrink-0 gap-1.5 border-t p-2.5">
        <button type="button" onClick={onNewTask} className={`${FOOTER_BUTTON} text-text-secondary`}>
          <Plus className="h-3.5 w-3.5" aria-hidden="true" />
          Nuovo task
        </button>
        <button type="button" onClick={onOpenIntake} className={`${FOOTER_BUTTON} bg-surface`}>
          <Sparkles className="h-3.5 w-3.5 text-primary" aria-hidden="true" />
          Task Intake
        </button>
      </footer>
    </aside>
  );
}
