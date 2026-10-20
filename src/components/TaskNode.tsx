import { memo } from "react";
import { Handle, Position } from "@xyflow/react";
import type { Node, NodeProps } from "@xyflow/react";
import { GitBranch, Pause, RotateCw } from "lucide-react";
import { cn } from "../lib/utils";
import {
  columnBorderClass,
  columnDotClass,
  columnLabel,
  columnTextClass,
  shortReport,
  workerSubStatus,
} from "../lib/board";
import type { Task } from "../lib/board";
import { NODE_HEIGHT, NODE_WIDTH } from "../lib/boardLayout";

export type TaskNodeData = { task: Task; selected: boolean } & Record<string, unknown>;
export type TaskFlowNode = Node<TaskNodeData, "task">;

/** Nodo del grafo: un task con colonna, sotto-stato del worker, branch e ultimo report. */
function TaskNode({ data }: NodeProps<TaskFlowNode>) {
  const { task, selected } = data;
  const sub = workerSubStatus(task);
  const report = shortReport(task);
  return (
    <div
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      aria-label={`${task.title} (${columnLabel(task.column)})`}
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
        columnBorderClass(task.column),
        selected && "ring-1 ring-ring",
        task.status === "cancelled" && "opacity-60",
      )}
    >
      <Handle type="target" position={Position.Left} className="!h-1.5 !w-1.5 !border-0 !bg-border-strong" />
      <Handle type="source" position={Position.Right} className="!h-1.5 !w-1.5 !border-0 !bg-border-strong" />

      <div className="flex items-center gap-1.5">
        <span className={cn("h-2 w-2 flex-shrink-0 rounded-full", columnDotClass(task.column))} />
        <span className={cn("text-[11px] font-medium", columnTextClass(task.column))}>
          {columnLabel(task.column)}
        </span>
        {sub && <span className={cn("text-[11px]", sub.className)}>· {sub.text}</span>}
        <span className="ml-auto flex items-center gap-1.5">
          {task.held && (
            <span className="flex items-center gap-0.5 text-[10px] text-status-attention">
              <Pause className="!size-2.5" />
              In pausa
            </span>
          )}
          {task.attempts > 0 && (
            <span
              className="flex items-center gap-0.5 font-mono text-[10px] text-muted-foreground"
              title={`Tentativi: ${task.attempts}`}
            >
              <RotateCw className="!size-2.5" />
              {task.attempts}
            </span>
          )}
        </span>
      </div>

      <p className="truncate text-[13px] font-medium text-foreground" title={task.title}>
        {task.title}
      </p>
      <p className="truncate font-mono text-[10px] text-faint">{task.id}</p>

      {task.worktree?.branch && (
        <p
          className="flex items-center gap-1 truncate font-mono text-[10px] text-muted-foreground"
          title={task.worktree.branch}
        >
          <GitBranch className="!size-2.5 flex-shrink-0" />
          <span className="truncate">{task.worktree.branch}</span>
        </p>
      )}
      {report && (
        <p className="line-clamp-2 text-[11px] leading-tight text-text-secondary" title={task.last_report ?? ""}>
          {report}
        </p>
      )}
    </div>
  );
}

export default memo(TaskNode);
