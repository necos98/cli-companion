import { memo } from "react";
import type { DragEvent } from "react";
import { TriangleAlert } from "lucide-react";
import { cn } from "../../lib/utils";
import AgentBadge from "./AgentBadge";
import BotBadge from "./BotBadge";
import { ReviewBadge, VerdictBadge } from "./ReviewBadges";
import { TaskE2eBadge } from "./E2eBadges";
import { isReview } from "../../lib/review";
import { isPlan } from "../../lib/plan";
import { PlanBadge } from "./PlanBadge";
import { blockedByLabel, displayStatus, failReasonLabel, queueHoldLabel, workerSubStatus } from "../../lib/board";
import type { Task } from "../../lib/board";
import { agentBadge, unavailableText } from "../../lib/taskAgent";
import type { TaskAgents } from "../../lib/taskAgent";
import { dragIntent, kanbanMeta } from "../../lib/taskViews";

export type DropEdge = "before" | "after" | null;

interface Props {
  task: Task;
  /** Tutti i task: serve al titolo del task da cui dipende. */
  tasks: Task[];
  /** Agenti del workspace: badge e motivo di un agente non pronto. */
  agents: TaskAgents;
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
  agents,
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
  const waiting = queueHoldLabel(task, tasks);
  const blocked = blockedByLabel(task, tasks) !== null;
  const failReason = failReasonLabel(task);
  const sub = attention ? null : workerSubStatus(task);
  const unavailable = unavailableText(agents, task.agent_unavailable);
  const draggable = dragIntent(task) !== null;
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
        attention ? "border-status-attention/50" : task.status === "failed" ? "border-destructive/40" : "border-border",
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
        {isReview(task) && <ReviewBadge />}
        {isPlan(task) && <PlanBadge />}
        <VerdictBadge task={task} />
        <TaskE2eBadge task={task} />
        <BotBadge bots={agents.bots} id={task.bot} />
        <AgentBadge name={agentBadge(agents, task.agent)} />
        {waiting && (
          <span
            className={cn(
              "max-w-full truncate rounded border px-1.5 py-px",
              blocked ? "border-destructive/40 text-destructive" : "border-border-strong",
            )}
          >
            {waiting}
          </span>
        )}
        {failReason && (
          <span className="max-w-full truncate rounded border border-destructive/40 px-1.5 py-px text-destructive">
            {failReason}
          </span>
        )}
        {unavailable && <span className="max-w-full truncate font-medium text-status-running">{unavailable}</span>}
        {showRepo && (
          <span className="max-w-[10rem] truncate rounded bg-surface-2 px-1.5 py-px font-mono">{task.repo || "."}</span>
        )}
        {sub && <span className={sub.className}>{sub.text}</span>}
        <span className="truncate">{kanbanMeta(task, now).join(" · ")}</span>
      </span>
    </button>
  );
}

export default memo(KanbanCard);
