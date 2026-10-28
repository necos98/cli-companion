import { useCallback, useState } from "react";
import type { DragEvent } from "react";
import { GripVertical } from "lucide-react";
import { waitingOnLabel } from "../../lib/board";
import type { Task } from "../../lib/board";
import { dropSlot, moveBeforeId, queueSubtitle } from "../../lib/taskPanel";
import { cn } from "@/lib/utils";
import { ReviewBadge } from "./ReviewBadges";
import { isReview } from "../../lib/review";

interface PanelQueueProps {
  /** Task Da fare nell'ordine della coda. */
  queue: Task[];
  /** Tutti i task della board (per i titoli di "attende X"). */
  allTasks: Task[];
  onOpen: (taskId: string) => void;
  /** Sposta `taskId` prima di `beforeId` (null = in fondo). */
  onMove: (taskId: string, beforeId: string | null) => void;
}

const DRAG_TYPE = "text/plain";

/** Coda numerata, riordinabile con il drag & drop HTML5 (linea di rilascio in primo piano). */
export default function PanelQueue({ queue, allTasks, onOpen, onMove }: PanelQueueProps) {
  const [dragId, setDragId] = useState<string | null>(null);
  const [slot, setSlot] = useState<number | null>(null);

  const reset = useCallback(() => {
    setDragId(null);
    setSlot(null);
  }, []);

  const ids = queue.map((t) => t.id);
  // Lo slot è valido solo se produce davvero uno spostamento.
  const indicator =
    dragId !== null && slot !== null && moveBeforeId(ids, dragId, slot) !== undefined ? slot : null;

  const onDragOver = (e: DragEvent<HTMLDivElement>, index: number) => {
    if (dragId === null) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    const rect = e.currentTarget.getBoundingClientRect();
    const next = dropSlot(index, e.clientY > rect.top + rect.height / 2);
    if (next !== slot) setSlot(next);
  };

  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    if (dragId !== null && slot !== null) {
      const before = moveBeforeId(ids, dragId, slot);
      if (before !== undefined) onMove(dragId, before);
    }
    reset();
  };

  const line = (edge: "top" | "bottom") => (
    <div
      aria-hidden="true"
      className={cn("pointer-events-none absolute inset-x-0 h-0.5 rounded bg-primary", edge === "top" ? "-top-px" : "-bottom-px")}
    />
  );

  return (
    <ul
      className="flex min-w-0 flex-col"
      onDragLeave={(e) => {
        // Fuori dalla lista: nessuna linea (tra due righe relatedTarget è ancora dentro).
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setSlot(null);
      }}
    >
      {queue.map((t, i) => (
        <li key={t.id} className="relative min-w-0">
          {indicator === i && line("top")}
          <div
            draggable
            onDragStart={(e) => {
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData(DRAG_TYPE, t.id);
              setDragId(t.id);
            }}
            onDragEnd={reset}
            onDragOver={(e) => onDragOver(e, i)}
            onDrop={onDrop}
            className={cn(
              "flex min-w-0 items-center gap-2.5 rounded-md px-1.5 py-[7px] transition-colors hover:bg-surface-2",
              dragId === t.id && "opacity-40",
            )}
          >
            <GripVertical className="h-3 w-3 flex-shrink-0 cursor-grab text-faint" aria-hidden="true" />
            <span className="w-3 flex-shrink-0 font-mono text-[11px] text-muted-foreground">{i + 1}</span>
            <QueueRowButton task={t} allTasks={allTasks} onOpen={onOpen} />
          </div>
          {indicator === queue.length && i === queue.length - 1 && line("bottom")}
        </li>
      ))}
    </ul>
  );
}

function QueueRowButton({ task, allTasks, onOpen }: { task: Task; allTasks: Task[]; onOpen: (id: string) => void }) {
  const waiting = waitingOnLabel(task, allTasks);
  const sub = queueSubtitle(task, waiting);
  return (
    <button
      type="button"
      onClick={() => onOpen(task.id)}
      title={sub ? `${task.title}\n${sub}` : task.title}
      className="flex min-w-0 flex-1 flex-col gap-0.5 rounded text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      <span className="flex min-w-0 items-center gap-1.5">
        <span className="truncate text-[13px] text-text-secondary">{task.title}</span>
        {isReview(task) && <ReviewBadge className="text-[10px]" />}
      </span>
      {sub && <span className="truncate text-[11px] text-muted-foreground">{sub}</span>}
    </button>
  );
}
