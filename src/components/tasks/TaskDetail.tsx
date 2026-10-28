import { useEffect, useMemo, useState } from "react";
import { X } from "lucide-react";
import DetailActions from "./DetailActions";
import DetailE2e from "./DetailE2e";
import DetailFacts from "./DetailFacts";
import DetailResult from "./DetailResult";
import DetailSection from "./DetailSection";
import Markdown from "./Markdown";
import { ReviewBadge, VerdictBadge } from "./ReviewBadges";
import { TaskE2eBadge } from "./E2eBadges";
import TaskEditForm from "./TaskEditForm";
import { useCommand } from "./useDetailCommand";
import {
  canEditTask,
  canOpenTerminal,
  displayStatus,
  statusBorderClass,
  statusDotClass,
  statusLabel,
  statusTextClass,
} from "../../lib/board";
import type { BoardView, Task, TaskAction } from "../../lib/board";
import { isReview } from "../../lib/review";
import { isMultiRepo, repoOptions } from "../../lib/taskForm";
import { openExternal } from "../../lib/openExternal";
import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import { cn } from "../../lib/utils";

const NO_REPOS: string[] = [];

/**
 * Props del dettaglio di un task (pannello laterale destro agganciato nel Tasks
 * Manager, uguale per Kanban / Lista / Dipendenze). Stato, azioni per stato,
 * esito, specifica in markdown, dipendenze e modifica (solo Da fare / Fallito)
 * vivono qui; chiama `api` e passa a `onBoard` la board restituita.
 */
export interface TaskDetailProps {
  workspaceId: string;
  task: Task;
  /** Board corrente: titoli delle dipendenze, dipendenti, candidati per la modifica. */
  board: BoardView;
  onBoard: (board: BoardView) => void;
  onClose: () => void;
  /** Torna ai terminali e mette a fuoco il tab del worker (`task.session`). */
  onOpenTerminal: (tabId: string) => void;
  /** Click su una dipendenza: il manager seleziona quel task. */
  onSelectTask: (taskId: string) => void;
  /** Repository rilevati del workspace (più quelli dei task); assente = solo quelli dei task. */
  repos?: string[];
  /** Configurazione per repository del workspace (`Workspace.repos`): dice dove gli E2E sono configurati. */
  repoConfigs?: Workspace["repos"];
  /** Modello predefinito dei worker, come segnaposto del campo modello. */
  defaultModel?: string | null;
  /** Il tab del worker esiste ancora? Assente = si assume di sì. */
  isTabOpen?: (tabId: string) => boolean;
  /** Orologio condiviso del manager (durata di un task in corso). */
  now?: number;
}

/** Dettaglio del task: intestazione con stato e azioni, poi esito, nota, specifica e dati. */
export default function TaskDetail({
  workspaceId,
  task,
  board,
  onBoard,
  onClose,
  onOpenTerminal,
  onSelectTask,
  repos = NO_REPOS,
  repoConfigs,
  defaultModel = null,
  isTabOpen,
  now,
}: TaskDetailProps) {
  const [editing, setEditing] = useState(false);
  const { run, busy, error, clearError } = useCommand(onBoard);
  const status = displayStatus(task);
  const clock = now ?? Date.now();
  const repoChoices = useMemo(() => repoOptions(board.tasks, repos), [board.tasks, repos]);

  // Se il task parte (o cambia stato) mentre lo si modifica, si esce dalla modifica.
  useEffect(() => {
    if (editing && !canEditTask(task)) setEditing(false);
  }, [editing, task]);

  const session = task.session ?? "";
  const tabAvailable =
    canOpenTerminal(task) && (task.status === "running" || (isTabOpen ? isTabOpen(session) : true));

  const act = (action: TaskAction) => void run(() => api.taskAction(workspaceId, task.id, action));

  return (
    <aside
      aria-label="Dettaglio del task"
      className="flex w-[440px] max-w-[50%] flex-shrink-0 flex-col border-l bg-base"
    >
      <div className="flex flex-col gap-2.5 border-b px-[18px] pb-3 pt-4">
        <div className="flex items-center gap-2">
          <span
            className={cn(
              "flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs",
              statusBorderClass(status),
              statusTextClass(status),
            )}
          >
            <span className={cn("h-1.5 w-1.5 rounded-full", statusDotClass(status))} aria-hidden="true" />
            {statusLabel(status)}
          </span>
          {isReview(task) && <ReviewBadge className="text-xs" />}
          <VerdictBadge task={task} className="text-xs" />
          <TaskE2eBadge task={task} className="text-xs" />
          <span className="min-w-0 truncate font-mono text-[11px] text-muted-foreground" title={task.id}>
            {task.id}
          </span>
          <button
            type="button"
            aria-label="Chiudi il dettaglio"
            onClick={onClose}
            className="ml-auto flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-md text-text-secondary hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <h2 className="break-words text-lg font-semibold leading-snug">{task.title}</h2>
        {!editing && (
          <DetailActions
            task={task}
            board={board}
            busy={busy}
            canOpenTerminal={tabAvailable}
            onEdit={() => {
              clearError();
              setEditing(true);
            }}
            onAction={act}
            onOpenTerminal={() => onOpenTerminal(session)}
            onOpenPr={openExternal}
          />
        )}
        {error && (
          <p role="alert" className="break-words text-xs text-destructive">
            {error}
          </p>
        )}
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-[18px] overflow-y-auto px-[18px] py-4">
        {editing ? (
          <TaskEditForm
            workspaceId={workspaceId}
            task={task}
            board={board}
            onBoard={onBoard}
            repoChoices={repoChoices}
            repoConfigs={repoConfigs}
            modelPlaceholder={defaultModel || "predefinito dei worker"}
            onDone={() => setEditing(false)}
          />
        ) : (
          <>
            <DetailResult task={task} />
            <DetailE2e workspaceId={workspaceId} task={task} />
            {task.note && (
              <DetailSection title="Nota">
                <p
                  className={cn(
                    "whitespace-pre-line break-words text-[13px] leading-relaxed",
                    task.status === "failed" ? "text-destructive" : "text-text-secondary",
                  )}
                >
                  {task.note}
                </p>
              </DetailSection>
            )}
            {task.previous_attempt && (task.previous_attempt.summary || task.previous_attempt.note) && (
              <DetailSection title="Tentativo precedente">
                <div className="flex flex-col gap-1 rounded-md border bg-surface px-3 py-2 text-xs text-text-secondary">
                  {task.previous_attempt.summary && (
                    <p className="whitespace-pre-line break-words">{task.previous_attempt.summary}</p>
                  )}
                  {task.previous_attempt.note && (
                    <p className="whitespace-pre-line break-words text-muted-foreground">
                      {task.previous_attempt.note}
                    </p>
                  )}
                </div>
              </DetailSection>
            )}
            <DetailSection title="Specifica">
              <Markdown source={task.task} />
            </DetailSection>
            <DetailFacts
              task={task}
              board={board}
              now={clock}
              showRepo={isMultiRepo(repoChoices)}
              onSelectTask={onSelectTask}
            />
          </>
        )}
      </div>
    </aside>
  );
}
