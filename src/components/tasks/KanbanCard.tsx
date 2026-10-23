import { memo } from "react";
import type { DragEvent } from "react";
import { TriangleAlert } from "lucide-react";
import { cn } from "../../lib/utils";
import { displayStatus, waitingOnLabel, workerSubStatus } from "../../lib/board";
import type { Task } from "../../lib/board";
import { isDraggable, kanbanMeta } from "../../lib/taskViews";

export type DropEdge = "before" | "after" | null;

interface Props {
  task: Task;
  /** Tutti i task: serve al titolo del task da cui dipende. */
  tasks: Task[];
  now: number;
  selected: boolean;
  /** Mostra il repository (workspace multi-repo). */
  showRepo: boolean;
  /** Dove cadrebbe il task trascinato rispetto a questa card. */
  dropEdge: DropEdge;
  dragging: boolean;
  onSelect: (id: string) => void;
  onDragStart: (e: DragEvent<HTMLButtonElement>, task: Task) => void;
  onDragEnd: () => void;
  onDragOver?: (e: DragEvent<HTMLButtonElement>, task: Task) => void;
}

const EDGE_SHADOW: Record<"before" | "after", string> = {
  before: "shadow-[0_-4px_0_0_var(--primary)]",
  after: "shadow-[0_4px_0_0_var(--primary)]",
};

/** Card di un task nel Kanban: titolo (2 righe), meta, "attende X", "Richiede attenzione". */
function KanbanCard({
  task,
  tasks,
  now,
  selected,
  showRepo,
  dropEdge,
  dragging,
  onSelect,
  onDragStart,
  onDragEnd,
  onDragOver,
}: Props) {
  const status = displayStatus(task);
  const attention = status === "attention";
  const waiting = waitingOnLabel(task, tasks);
  const sub = attention ? null : workerSubStatus(task);
  const draggable = isDraggable(task.status);
  return (
    <button
      type="button"
      draggable={draggable}
      aria-pressed={selected}
      onClick={() => onSelect(task.id)}
      onDragStart={draggable ? (e) => onDragStart(e, task) : undefined}
      onDragEnd={draggable ? onDragEnd : undefined}
      onDragOver={onDragOver ? (e) => onDragOver(e, task) : undefined}
      className={cn(
        "flex w-full flex-shrink-0 flex-col gap-2 rounded-lg border bg-surface px-3 py-[11px] text-left text-foreground transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        attention
          ? "border-status-attention/50"
          : task.status === "failed"
            ? "border-destructive/40"
            : "border-border",
        selected && "bg-surface-2 ring-1 ring-primary",
        draggable && "cursor-grab active:cursor-grabbing",
        dragging && "opacity-40",
        dropEdge && EDGE_SHADOW[dropEdge],
      )}
    >
      <span className="line-clamp-2 break-words text-[13px] font-medium leading-snug">{task.title}</span>
      <span className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-1 text-[11px] text-muted-foreground">
        {attention && (
          <span className="flex items-center gap-1 font-medium text-status-attention">
            <TriangleAlert className="h-3 w-3" aria-hidden="true" />
            Richiede attenzione
          </span>
        )}
        {waiting && (
          <span className="max-w-full truncate rounded border border-border-strong px-1.5 py-px">{waiting}</span>
        )}
        {showRepo && (
          <span className="max-w-[10rem] truncate rounded bg-surface-2 px-1.5 py-px font-mono">
            {task.repo || "."}
          </span>
        )}
        {sub && <span className={sub.className}>{sub.text}</span>}
        <span className="truncate">{kanbanMeta(task, now).join(" · ")}</span>
      </span>
    </button>
  );
}

export default memo(KanbanCard);
