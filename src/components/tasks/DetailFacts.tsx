import type { ReactNode } from "react";
import BotBadge from "./BotBadge";
import { NO_BOT_TEXT } from "../../lib/taskBot";
import type { AgentBot } from "../../bindings/AgentBot";
import { cn } from "../../lib/utils";
import { formatTaskDuration, statusDotClass } from "../../lib/board";
import type { BoardView, Task } from "../../lib/board";

function Fact({ label, children, wide = false }: { label: string; children: ReactNode; wide?: boolean }) {
  return (
    <div className={cn("flex min-w-0 flex-col gap-1", wide && "col-span-2")}>
      <span className="text-[11px] text-muted-foreground">{label}</span>
      <div className="min-w-0 break-words text-[13px]">{children}</div>
    </div>
  );
}

interface Props {
  task: Task;
  board: BoardView;
  now: number;
  /** Bot noti: nome e avatar di quello del task. */
  bots: readonly AgentBot[];
  /** Mostra il repository anche se è la root (workspace multi-repo). */
  showRepo: boolean;
  onSelectTask: (taskId: string) => void;
}

/** Dipendenze (cliccabili), Bot, modello, effort, skill, repo, durata e tentativi. */
export default function DetailFacts({ task, board, now, bots, showRepo, onSelectTask }: Props) {
  const deps = task.depends_on;
  const duration = formatTaskDuration(task, now);
  return (
    <div className="grid grid-cols-2 gap-x-3 gap-y-3 border-t pt-4">
      <Fact label="Dipende da" wide>
        {deps.length === 0 ? (
          <span className="text-muted-foreground">—</span>
        ) : (
          <ul className="flex flex-wrap gap-1.5">
            {deps.map((id) => {
              const dep = board.tasks.find((t) => t.id === id);
              return (
                <li key={id} className="min-w-0 max-w-full">
                  <button
                    type="button"
                    disabled={!dep}
                    onClick={() => onSelectTask(id)}
                    title={dep ? `${dep.title} (${id})` : id}
                    className="flex max-w-full items-center gap-1.5 rounded-md border border-border-strong px-2 py-0.5 text-xs hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60"
                  >
                    <span
                      className={cn("h-1.5 w-1.5 flex-shrink-0 rounded-full", statusDotClass(dep?.status ?? "queued"))}
                      aria-hidden="true"
                    />
                    <span className="truncate">{dep?.title ?? id}</span>
                    {dep?.status === "done" && <span className="sr-only"> (fatto)</span>}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </Fact>
      <Fact label="Bot">
        {task.bot ? <BotBadge bots={bots} id={task.bot} className="max-w-full text-xs" /> : NO_BOT_TEXT}
      </Fact>
      <Fact label="Modello">
        {task.model ? task.model : <span className="text-muted-foreground">predefinito</span>}
      </Fact>
      {task.effort && <Fact label="Effort">{task.effort}</Fact>}
      {task.skill && (
        <Fact label="Skill">
          <span className="font-mono text-xs">{task.skill.id}</span>
        </Fact>
      )}
      {(showRepo || task.repo) && (
        <Fact label="Repository">
          <span className="font-mono text-xs">{task.repo || "."}</span>
        </Fact>
      )}
      <Fact label="Durata">{duration || <span className="text-muted-foreground">—</span>}</Fact>
      <Fact label="Tentativi">{task.attempts}</Fact>
    </div>
  );
}
