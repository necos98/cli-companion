import { X } from "lucide-react";
import DetailSection from "./DetailSection";
import GoalCloseBox from "./GoalCloseBox";
import GoalClosedInfo from "./GoalClosedInfo";
import GoalDecisions from "./GoalDecisions";
import GoalProgressLine from "./GoalProgressLine";
import GoalReviews from "./GoalReviews";
import GoalRounds from "./GoalRounds";
import type { BoardView } from "../../lib/board";
import { goalTasks, goalWaitingLabels, isGoalOpen } from "../../lib/goals";
import type { GoalView } from "../../lib/goals";

/**
 * Props del dettaglio di un goal (stesso pannello laterale destro del task): richiesta, criteri, decisioni
 * con la nota, task per giro, esito delle review e chiusura. Chiama `api` e passa a `onBoard` la board restituita.
 */
export interface GoalDetailProps {
  workspaceId: string;
  goal: GoalView;
  board: BoardView;
  now: number;
  onBoard: (board: BoardView) => void;
  onClose: () => void;
  /** Click su un task del goal: il manager seleziona quel task (chiudendo il goal). */
  onSelectTask: (taskId: string) => void;
}

/** Dettaglio del goal: intestazione con fase e avanzamento, poi ciò che serve per decidere. */
export default function GoalDetail({ workspaceId, goal, board, now, onBoard, onClose, onSelectTask }: GoalDetailProps) {
  const open = isGoalOpen(goal);
  const waiting = goalWaitingLabels(goal);
  const tasks = goalTasks(board.tasks, goal.id);
  return (
    <aside
      aria-label="Dettaglio del goal"
      className="flex w-[440px] max-w-[50%] flex-shrink-0 flex-col border-l bg-base"
    >
      <div className="flex flex-col gap-2.5 border-b px-[18px] pb-3 pt-4">
        <div className="flex items-center gap-2">
          <span className="rounded-full border border-border-strong px-2 py-0.5 text-xs">
            <GoalProgressLine goal={goal} />
          </span>
          <span className="min-w-0 truncate font-mono text-[11px] text-muted-foreground" title={goal.id}>
            {goal.id}
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
        <h2 className="break-words text-lg font-semibold leading-snug">{goal.title}</h2>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-[18px] overflow-y-auto px-[18px] py-4">
        {!open && <GoalClosedInfo goal={goal} now={now} />}
        {open && waiting.length > 0 && (
          <DetailSection title="Ti aspetta">
            <ul className="list-disc pl-4 text-[13px] text-status-attention">
              {waiting.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          </DetailSection>
        )}
        <DetailSection title="Richiesta">
          <p className="whitespace-pre-line break-words text-[13px] leading-relaxed">{goal.request}</p>
        </DetailSection>
        {goal.done_when.length > 0 && (
          <DetailSection title="Fatto quando">
            <ul className="list-disc pl-4 text-[13px] leading-relaxed">
              {goal.done_when.map((c, i) => (
                <li key={`${i}-${c}`} className="break-words">
                  {c}
                </li>
              ))}
            </ul>
          </DetailSection>
        )}
        <GoalDecisions workspaceId={workspaceId} goal={goal} open={open} now={now} onBoard={onBoard} />
        <GoalRounds tasks={tasks} onSelectTask={onSelectTask} />
        <GoalReviews tasks={tasks} onSelectTask={onSelectTask} />
        {open && <GoalCloseBox workspaceId={workspaceId} goal={goal} board={board} onBoard={onBoard} />}
      </div>
    </aside>
  );
}
