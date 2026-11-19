import DetailSection from "./DetailSection";
import { VerdictBadge } from "./ReviewBadges";
import { displayStatus, statusDotClass, statusLabel } from "../../lib/board";
import type { Task } from "../../lib/board";
import { roundAcceptanceLabel } from "../../lib/autoFix";
import { groupByRound } from "../../lib/goals";
import type { GoalView } from "../../lib/goals";
import { cn } from "../../lib/utils";

interface Props {
  goal: Pick<GoalView, "auto_rounds">;
  /** Task del goal, nell'ordine della board. */
  tasks: Task[];
  onSelectTask: (taskId: string) => void;
}

/** "Giro N", con chi l'ha accettato se è un giro di correzione. */
function RoundHeading({ round, accepted }: { round: number; accepted: string | null }) {
  return (
    <h4 className="text-xs font-medium text-text-secondary">
      Giro {round}
      {accepted && <span className="font-normal text-muted-foreground"> · {accepted}</span>}
    </h4>
  );
}

/**
 * Task del goal per giro: "Giro N" (dal secondo, chi l'ha accettato: l'app o l'utente), poi titolo, stato e verdetto
 * di ognuno; il clic apre il task.
 */
export default function GoalRounds({ goal, tasks, onSelectTask }: Props) {
  return (
    <DetailSection title="Task">
      {tasks.length === 0 ? (
        <p className="text-xs text-muted-foreground">Nessun task ancora.</p>
      ) : (
        groupByRound(tasks).map((group) => (
          <div key={group.round} className="flex flex-col gap-1">
            <RoundHeading round={group.round} accepted={roundAcceptanceLabel(goal, group.round)} />
            <ul className="flex flex-col gap-1">
              {group.tasks.map((t) => {
                const status = displayStatus(t);
                return (
                  <li key={t.id}>
                    <button
                      type="button"
                      onClick={() => onSelectTask(t.id)}
                      className="flex w-full min-w-0 items-center gap-2 rounded-md border border-border-strong px-2.5 py-1.5 text-left text-xs hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    >
                      <span
                        className={cn("h-1.5 w-1.5 flex-shrink-0 rounded-full", statusDotClass(status))}
                        aria-hidden="true"
                      />
                      <span className="min-w-0 flex-1 truncate">{t.title}</span>
                      <VerdictBadge task={t} className="text-[11px]" />
                      <span className="flex-shrink-0 text-muted-foreground">{statusLabel(status)}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ))
      )}
    </DetailSection>
  );
}
