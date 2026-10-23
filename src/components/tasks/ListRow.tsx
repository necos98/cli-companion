import { memo } from "react";
import { TriangleAlert } from "lucide-react";
import { cn } from "../../lib/utils";
import {
  displayStatus,
  formatRelative,
  formatTaskDuration,
  statusDotClass,
  statusLabel,
  statusTextClass,
} from "../../lib/board";
import type { Task } from "../../lib/board";

/** Colonne della Lista: Stato, Titolo, Modello, Aggiornato, Durata. */
export const LIST_GRID = "grid grid-cols-[150px_minmax(220px,1fr)_96px_110px_80px] gap-3";

interface Props {
  task: Task;
  subtitle: string;
  now: number;
  selected: boolean;
  /** Mostra il repository davanti al sottotitolo (workspace multi-repo). */
  showRepo: boolean;
  onSelect: (id: string) => void;
}

/** Riga della Lista: un bottone (tastiera e lettori di schermo) con le celle in griglia. */
function ListRow({ task, subtitle, now, selected, showRepo, onSelect }: Props) {
  const status = displayStatus(task);
  const sub = [showRepo ? task.repo || "." : null, subtitle].filter(Boolean).join(" · ");
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={() => onSelect(task.id)}
      className={cn(
        LIST_GRID,
        "w-full min-w-[700px] items-center border-b border-border/60 px-4 py-2.5 text-left text-[13px] transition-colors hover:bg-surface-2/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
        selected && "bg-surface-2",
      )}
    >
      <span className={cn("flex min-w-0 items-center gap-[7px]", statusTextClass(status))}>
        {status === "attention" ? (
          <TriangleAlert className="h-3 w-3 flex-shrink-0" aria-hidden="true" />
        ) : (
          <span className={cn("h-[7px] w-[7px] flex-shrink-0 rounded-full", statusDotClass(status))} aria-hidden="true" />
        )}
        <span className="truncate">{statusLabel(status)}</span>
      </span>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className="truncate">{task.title}</span>
        {sub && <span className="truncate text-[11px] text-muted-foreground">{sub}</span>}
      </span>
      <span className="truncate text-text-secondary">{task.model || "—"}</span>
      <span className="truncate text-text-secondary">{formatRelative(task.updated_at, now)}</span>
      <span className="font-mono text-xs text-text-secondary">{formatTaskDuration(task, now) || "—"}</span>
    </button>
  );
}

export default memo(ListRow);
