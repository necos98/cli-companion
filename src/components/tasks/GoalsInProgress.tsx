import { useMemo } from "react";
import { goalPhaseLabel, goalProgressLabel, goalRoundLabel, isWaiting, openGoals } from "../../lib/goals";
import type { GoalView } from "../../lib/goals";
import PanelSection from "./PanelSection";

interface GoalsInProgressProps {
  goals: readonly GoalView[];
  onOpenGoal: (goalId: string) => void;
}

/** Sezione "GOAL IN CORSO": un pulsante per goal aperto, con fase, giro e avanzamento. Assente se non ce ne sono. */
export default function GoalsInProgress({ goals, onOpenGoal }: GoalsInProgressProps) {
  const open = useMemo(() => openGoals(goals), [goals]);
  if (open.length === 0) return null;
  return (
    <PanelSection title="GOAL IN CORSO" count={open.length}>
      {open.map((goal) => {
        const round = goalRoundLabel(goal);
        const phase = [goalPhaseLabel(goal), round].filter(Boolean).join(" · ");
        return (
          <button
            key={goal.id}
            type="button"
            onClick={() => onOpenGoal(goal.id)}
            aria-label={`Apri il goal ${goal.title}`}
            title={goal.title}
            className="flex w-full min-w-0 items-center gap-2.5 rounded-lg border border-border bg-surface px-2.5 py-[9px] text-left transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
              <span className="line-clamp-2 break-words text-[13px] font-medium">{goal.title}</span>
              <span className="truncate text-[11px] text-muted-foreground">{phase}</span>
            </span>
            {isWaiting(goal) && (
              <span className="flex flex-shrink-0 items-center gap-1 text-[11px] text-primary">
                <span className="h-1.5 w-1.5 rounded-full bg-primary" aria-hidden="true" />
                Serve a te
              </span>
            )}
            <span className="flex-shrink-0 font-mono text-[11px] text-muted-foreground">{goalProgressLabel(goal)}</span>
          </button>
        );
      })}
    </PanelSection>
  );
}
