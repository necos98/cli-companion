import { useCallback, useMemo, useState } from "react";
import type { DragEvent } from "react";
import KanbanCard from "./KanbanCard";
import KanbanColumn from "./KanbanColumn";
import { useCommand } from "./useDetailCommand";
import { isParallel, statusDotClass } from "../../lib/board";
import type { BoardView, Task, TaskStatus } from "../../lib/board";
import { acceptsDrop, kanbanColumns, reorderTarget } from "../../lib/taskViews";
import { api } from "../../lib/tauri";

interface Props {
  workspaceId: string;
  board: BoardView;
  query: string;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onBoard: (board: BoardView) => void;
  /** Workspace con più repository: le card mostrano il repo. */
  showRepo: boolean;
  now: number;
  /** "Vedi tutti i fatti nella Lista →". */
  onShowAllDone: () => void;
}

interface Drag {
  id: string;
  status: TaskStatus;
}

/** Posizione di rilascio sopra/sotto una card della coda. */
interface Hover {
  id: string;
  after: boolean;
}

const EMPTY_TEXT = "Nessun task";

/**
 * Kanban: Da fare (ordine di partenza), In corso, Fatto (ultimi 7 giorni),
 * Fallito (solo se non vuota). Drag & drop HTML5: riordino dentro Da fare
 * (`task_move`) e Fallito → Da fare = Riprova.
 */
export default function KanbanView({
  workspaceId,
  board,
  query,
  selectedId,
  onSelect,
  onBoard,
  showRepo,
  now,
  onShowAllDone,
}: Props) {
  const cols = useMemo(() => kanbanColumns(board.tasks, query, now), [board.tasks, query, now]);
  const queueIds = useMemo(
    () => board.tasks.filter((t) => t.status === "queued").map((t) => t.id),
    [board.tasks],
  );
  const runningTotal = useMemo(() => board.tasks.filter((t) => t.status === "running").length, [board.tasks]);
  const hasDone = useMemo(() => board.tasks.some((t) => t.status === "done"), [board.tasks]);

  const { run, error, clearError } = useCommand(onBoard);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [hover, setHover] = useState<Hover | null>(null);

  const endDrag = useCallback(() => {
    setDrag(null);
    setHover(null);
  }, []);

  const onDragStart = useCallback(
    (e: DragEvent<HTMLButtonElement>, task: Task) => {
      e.dataTransfer.setData("text/plain", task.id);
      e.dataTransfer.effectAllowed = "move";
      clearError();
      setDrag({ id: task.id, status: task.status });
    },
    [clearError],
  );

  // Sopra una card della coda: dice se il rilascio cadrebbe prima o dopo di lei.
  const onCardDragOver = useCallback(
    (e: DragEvent<HTMLButtonElement>, task: Task) => {
      if (!drag || drag.status !== "queued" || task.status !== "queued") return;
      e.preventDefault();
      e.stopPropagation();
      e.dataTransfer.dropEffect = "move";
      if (task.id === drag.id) {
        setHover(null);
        return;
      }
      const r = e.currentTarget.getBoundingClientRect();
      const after = e.clientY > r.top + r.height / 2;
      setHover((prev) => (prev && prev.id === task.id && prev.after === after ? prev : { id: task.id, after }));
    },
    [drag],
  );

  const onQueueDragOver = (e: DragEvent<HTMLElement>) => {
    if (!drag || !acceptsDrop(drag.status, "queued")) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    if (hover) setHover(null);
  };

  const onQueueDrop = (e: DragEvent<HTMLElement>) => {
    e.preventDefault();
    const d = drag;
    const h = hover;
    endDrag();
    if (!d || !acceptsDrop(d.status, "queued")) return;
    if (d.status === "failed") {
      void run(() => api.taskAction(workspaceId, d.id, "retry"));
      return;
    }
    const target = reorderTarget(queueIds, d.id, h?.id ?? null, h?.after ?? true);
    if (target) void run(() => api.taskMove(workspaceId, d.id, target.beforeId));
  };

  const columnProps = (key: TaskStatus) => ({
    accepting: drag !== null && acceptsDrop(drag.status, key),
    dimmed: drag !== null && !acceptsDrop(drag.status, key) && key !== drag.status,
  });

  const card = (t: Task, queued = false) => (
    <KanbanCard
      key={t.id}
      task={t}
      tasks={board.tasks}
      now={now}
      selected={t.id === selectedId}
      showRepo={showRepo}
      dropEdge={queued && hover?.id === t.id ? (hover.after ? "after" : "before") : null}
      dragging={drag?.id === t.id}
      onSelect={onSelect}
      onDragStart={onDragStart}
      onDragEnd={endDrag}
      onDragOver={queued ? onCardDragOver : undefined}
    />
  );

  const empty = <p className="px-1 py-2 text-xs text-muted-foreground">{EMPTY_TEXT}</p>;
  const dropArea = drag && acceptsDrop(drag.status, "queued") && (
    <div className="rounded-lg border border-dashed border-primary px-3.5 py-3.5 text-center text-xs text-primary">
      {drag.status === "failed" ? "Rilascia qui per riprovare" : "Rilascia qui"}
    </div>
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      {error && (
        <p role="alert" className="flex-shrink-0 border-b px-4 py-1.5 text-xs text-destructive">
          {error}
        </p>
      )}
      <div className="flex min-h-0 flex-1 gap-3 overflow-x-auto p-4">
        <KanbanColumn
          label="Da fare"
          dotClass={statusDotClass("queued")}
          count={cols.queued.length}
          hint="in ordine di partenza"
          {...columnProps("queued")}
          onDragOver={onQueueDragOver}
          onDrop={onQueueDrop}
          footer={dropArea}
        >
          {cols.queued.map((t) => card(t, true))}
          {cols.queued.length === 0 && !dropArea && empty}
        </KanbanColumn>

        <KanbanColumn
          label="In corso"
          dotClass={statusDotClass("running")}
          count={cols.running.length}
          hint={isParallel(board.delivery) ? `${runningTotal} di ${board.max_parallel}` : undefined}
          {...columnProps("running")}
        >
          {cols.running.map((t) => card(t))}
          {cols.running.length === 0 && empty}
        </KanbanColumn>

        <KanbanColumn
          label="Fatto"
          dotClass={statusDotClass("done")}
          count={cols.done.length}
          hint="ultimi 7 giorni"
          {...columnProps("done")}
          footer={
            hasDone && (
              <button
                type="button"
                onClick={onShowAllDone}
                className="self-start rounded px-1 py-1.5 text-left text-xs text-primary hover:text-primary/80 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                Vedi tutti i fatti nella Lista →
              </button>
            )
          }
        >
          {cols.done.map((t) => card(t))}
          {cols.done.length === 0 && empty}
        </KanbanColumn>

        {cols.failed.length > 0 && (
          <KanbanColumn
            label="Fallito"
            dotClass={statusDotClass("failed")}
            count={cols.failed.length}
            {...columnProps("failed")}
          >
            {cols.failed.map((t) => card(t))}
          </KanbanColumn>
        )}
      </div>
    </div>
  );
}
