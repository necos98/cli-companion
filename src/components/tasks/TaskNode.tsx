import { memo } from "react";
import { Handle, Position } from "@xyflow/react";
import type { Node, NodeProps } from "@xyflow/react";
import { GitBranch, RotateCw, TriangleAlert } from "lucide-react";
import { cn } from "../../lib/utils";
import { ReviewBadge, VerdictBadge } from "./ReviewBadges";
import { isReview } from "../../lib/review";
import {
  displayStatus,
  shortReport,
  statusBorderClass,
  statusDotClass,
  statusLabel,
  statusTextClass,
  workerSubStatus,
} from "../../lib/board";
import type { Task } from "../../lib/board";
import { NODE_HEIGHT, NODE_WIDTH } from "../../lib/boardLayout";

export type TaskNodeData = { task: Task; selected: boolean } & Record<string, unknown>;
export type TaskFlowNode = Node<TaskNodeData, "task">;

/** Nodo del grafo: un task con stato (anche "Richiede attenzione"), branch e ultimo report. */
function TaskNode({ data }: NodeProps<TaskFlowNode>) {
  const { task, selected } = data;
  const status = displayStatus(task);
  const sub = status === "attention" ? null : workerSubStatus(task);
  const report = shortReport(task);
  return (
    <div
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      aria-label={`${task.title} (${statusLabel(status)})`}
      // Invio/Spazio come il clic: il click sintetico risale al nodo di React Flow.
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          e.currentTarget.click();
        }
      }}
      style={{ width: NODE_WIDTH, height: NODE_HEIGHT }}
      className={cn(
        "flex cursor-pointer flex-col gap-1 overflow-hidden rounded-[9px] border bg-surface px-2.5 py-2 text-left transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        statusBorderClass(status),
        selected && "ring-1 ring-ring",
      )}
    >
      <Handle type="target" position={Position.Left} className="!h-1.5 !w-1.5 !border-0 !bg-border-strong" />
      <Handle type="source" position={Position.Right} className="!h-1.5 !w-1.5 !border-0 !bg-border-strong" />

      <div className="flex items-center gap-1.5">
        {status === "attention" ? (
          <TriangleAlert className="!size-3 flex-shrink-0 text-status-attention" />
        ) : (
          <span className={cn("h-2 w-2 flex-shrink-0 rounded-full", statusDotClass(status))} />
        )}
        <span className={cn("text-[11px] font-medium", statusTextClass(status))}>
          {statusLabel(status)}
        </span>
        {sub && (
          <span className={cn("text-[11px]", sub.className)} title={task.preparing ?? undefined}>
            · {sub.text}
          </span>
        )}
        {isReview(task) && <ReviewBadge className="ml-1 text-[10px]" />}
        <VerdictBadge task={task} className="text-[10px]" />
        {task.attempts > 1 && (
          <span
            className="ml-auto flex items-center gap-0.5 font-mono text-[10px] text-muted-foreground"
            title={`Tentativi: ${task.attempts}`}
          >
            <RotateCw className="!size-2.5" />
            {task.attempts}
          </span>
        )}
      </div>

      <p className="truncate text-[13px] font-medium text-foreground" title={task.title}>
        {task.title}
      </p>
      <p className="flex items-center gap-1.5 truncate font-mono text-[10px] text-faint">
        <span className="truncate">{task.id}</span>
        {task.repo && (
          <span
            className="max-w-[45%] flex-shrink-0 truncate rounded border border-border-strong px-1 text-muted-foreground"
            title={`Repository: ${task.repo}`}
          >
            {task.repo}
          </span>
        )}
      </p>

      {task.worktree?.branch ? (
        <p
          className="flex items-center gap-1 truncate font-mono text-[10px] text-muted-foreground"
          title={task.worktree.branch}
        >
          <GitBranch className="!size-2.5 flex-shrink-0" />
          <span className="truncate">{task.worktree.branch}</span>
        </p>
      ) : (
        report && (
          <p className="truncate text-[11px] text-text-secondary" title={task.last_report ?? ""}>
            {report}
          </p>
        )
      )}
    </div>
  );
}

export default memo(TaskNode);
