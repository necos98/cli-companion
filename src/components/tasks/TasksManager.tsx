import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import DependencyGraph from "./DependencyGraph";
import KanbanView from "./KanbanView";
import ListView from "./ListView";
import ManagerHeader from "./ManagerHeader";
import ManagerProposals from "./ManagerProposals";
import NewTaskDialog from "./NewTaskDialog";
import TaskDetail from "./TaskDetail";
import { useCommand } from "./useDetailCommand";
import { useManagerNow } from "./useManagerNow";
import { useManagerPrefs } from "./useManagerPrefs";
import { useWorkspaceRepos } from "./useWorkspaceRepos";
import { clampMaxParallel, deliveryLabel } from "../../lib/board";
import type { BoardView, Task } from "../../lib/board";
import { repoOptions } from "../../lib/taskForm";
import { isTypingTarget } from "../../lib/taskViews";
import type { ManagerView } from "../../lib/taskViews";
import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";

/**
 * Richiesta di apertura che arriva da fuori (pannello Task): `nonce` cambia a
 * ogni richiesta, così il manager reagisce anche se è già aperto. App la
 * azzera quando il manager si apre dalla sidebar / Ctrl+Maiusc+B.
 * Il manager la applica in un effect su `intent.nonce` (all'apertura la legge
 * anche come stato iniziale).
 */
export interface TasksManagerIntent {
  nonce: number;
  /** Seleziona il task e apre il dettaglio. */
  taskId?: string;
  /** Apre il dialog "Nuovo task". */
  newTask?: boolean;
}

/**
 * Props del Tasks Manager (ex Board): copre l'area dei pannelli del workspace
 * (App lo monta sopra la riga pannello Task + griglia, quindi i TerminalView
 * restano montati). Montato solo mentre è aperto, con `key = workspace.id`:
 * lo stato locale (vista, ricerca) nasce a ogni apertura; i filtri della Lista
 * vanno ricordati per workspace in localStorage (con try/catch).
 */
export interface TasksManagerProps {
  workspace: Workspace;
  /** Coda corrente; null finché `board_get` non ha risposto. */
  board: BoardView | null;
  /** Errore del caricamento iniziale (`board_get` fallito). */
  loadError: string | null;
  /** Aggiorna la board in App con la risposta di un comando. */
  onBoard: (board: BoardView) => void;
  /** "← Terminali": chiude il manager. */
  onBackToTerminals: () => void;
  /** Torna ai terminali e mette a fuoco il tab del worker. */
  onOpenTerminal: (tabId: string) => void;
  /** Mette a fuoco il tab Task Intake del workspace, o ne apre uno nuovo. */
  onOpenIntake: () => void;
  intent?: TasksManagerIntent | null;
  /**
   * Il tab del worker esiste ancora? Serve a "Apri terminale" di un task
   * Fallito; assente = si assume di sì.
   */
  isTabOpen?: (tabId: string) => boolean;
}

/**
 * Tasks Manager: barra, viste Kanban | Lista | Dipendenze, proposte pendenti,
 * dettaglio agganciato a destra e dialog "Nuovo task".
 */
export default function TasksManager({
  workspace,
  board,
  loadError,
  onBoard,
  onBackToTerminals,
  onOpenTerminal,
  onOpenIntake,
  intent = null,
  isTabOpen,
}: TasksManagerProps) {
  const { prefs, setView, setList } = useManagerPrefs(workspace.id);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(intent?.taskId ?? null);
  const [newTaskOpen, setNewTaskOpen] = useState(intent?.newTask === true);
  const now = useManagerNow();
  const detected = useWorkspaceRepos(workspace.id);
  const searchRef = useRef<HTMLInputElement>(null);
  const header = useCommand(onBoard);

  const repoChoices = useMemo(() => repoOptions(board?.tasks ?? [], detected), [board?.tasks, detected]);
  const multiRepo = repoChoices.length > 1;
  const selected = board?.tasks.find((t) => t.id === selectedId) ?? null;

  // Richieste da fuori (pannello Task) anche a manager già aperto.
  useEffect(() => {
    if (!intent) return;
    if (intent.taskId) setSelectedId(intent.taskId);
    if (intent.newTask) setNewTaskOpen(true);
  }, [intent?.nonce]); // eslint-disable-line react-hooks/exhaustive-deps

  // Un task selezionato che sparisce dalla board (eliminato) chiude il dettaglio.
  // Solo se lo si era già visto: una selezione arrivata prima della board che lo contiene resta in attesa.
  const seenId = useRef<string | null>(null);
  useEffect(() => {
    if (!board || !selectedId) return;
    if (board.tasks.some((t) => t.id === selectedId)) seenId.current = selectedId;
    else if (seenId.current === selectedId) {
      seenId.current = null;
      setSelectedId(null);
    }
  }, [board, selectedId]);

  // `/` mette a fuoco la ricerca, se il fuoco non è in un campo di testo (né nel terminale).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return;
      if (isTypingTarget(e.target as HTMLElement | null) || newTaskOpen) return;
      e.preventDefault();
      searchRef.current?.focus();
      searchRef.current?.select();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [newTaskOpen]);

  const showAllDone = useCallback(() => {
    setList({ ...prefs.list, statuses: ["done"], repos: [] });
    setView("list");
    setQuery("");
  }, [prefs.list, setList, setView]);

  const onCreated = useCallback((task: Task | null) => {
    if (task) setSelectedId(task.id);
  }, []);

  const changeView = (v: ManagerView) => setView(v);

  return (
    <div className="flex h-full w-full min-w-0 flex-col bg-pane-area text-foreground">
      <ManagerHeader
        workspaceName={workspace.name}
        delivery={board ? deliveryLabel(board.delivery.mode) : null}
        board={board}
        view={prefs.view}
        onView={changeView}
        query={query}
        onQuery={setQuery}
        searchRef={searchRef}
        busy={header.busy}
        onBackToTerminals={onBackToTerminals}
        onMaxParallel={(n) => void header.run(() => api.boardSet(workspace.id, { maxParallel: clampMaxParallel(n) }))}
        onTogglePause={() => board && void header.run(() => api.boardSet(workspace.id, { paused: !board.paused }))}
        onNewTask={() => setNewTaskOpen(true)}
        onOpenIntake={onOpenIntake}
      />

      {(loadError || header.error) && (
        <p role="alert" className="flex-shrink-0 border-b px-4 py-1.5 text-xs text-destructive">
          {header.error ?? loadError}
        </p>
      )}
      {board?.schedule_blocked && (
        <p className="flex-shrink-0 border-b px-4 py-1.5 text-xs text-status-running">
          Fuori fascia oraria: nessun nuovo task parte finché la fascia non riapre.
        </p>
      )}
      {board?.paused && (
        <p className="flex-shrink-0 border-b px-4 py-1.5 text-xs text-muted-foreground">
          Coda in pausa: i task in corso finiscono, nessun nuovo task parte.
        </p>
      )}
      {board && <ManagerProposals workspaceId={workspace.id} board={board} onBoard={onBoard} />}

      <div className="flex min-h-0 flex-1">
        <main className="min-h-0 min-w-0 flex-1 overflow-hidden">
          {!board ? (
            <div className="flex h-full items-center justify-center p-6">
              <p className="text-[13px] text-muted-foreground">{loadError ? "Coda non disponibile." : "Caricamento…"}</p>
            </div>
          ) : prefs.view === "kanban" ? (
            <KanbanView
              workspaceId={workspace.id}
              board={board}
              query={query}
              selectedId={selectedId}
              onSelect={setSelectedId}
              onBoard={onBoard}
              showRepo={multiRepo}
              now={now}
              onShowAllDone={showAllDone}
            />
          ) : prefs.view === "list" ? (
            <ListView
              board={board}
              query={query}
              onQuery={setQuery}
              prefs={prefs.list}
              onPrefs={setList}
              repoChoices={repoChoices}
              selectedId={selectedId}
              onSelect={setSelectedId}
              now={now}
              onNewTask={() => setNewTaskOpen(true)}
            />
          ) : (
            <DependencyGraph board={board} selectedId={selectedId} onSelect={setSelectedId} />
          )}
        </main>
        {board && selected && (
          <TaskDetail
            key={selected.id}
            workspaceId={workspace.id}
            task={selected}
            board={board}
            onBoard={onBoard}
            onClose={() => setSelectedId(null)}
            onOpenTerminal={onOpenTerminal}
            onSelectTask={setSelectedId}
            repos={detected}
            defaultModel={workspace.worker_model}
            isTabOpen={isTabOpen}
            now={now}
          />
        )}
      </div>

      <NewTaskDialog
        open={newTaskOpen}
        workspace={workspace}
        board={board}
        onBoard={onBoard}
        onClose={() => setNewTaskOpen(false)}
        onOpenIntake={onOpenIntake}
        repos={detected}
        onCreated={onCreated}
      />
    </div>
  );
}
