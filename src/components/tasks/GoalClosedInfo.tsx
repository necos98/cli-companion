import DetailSection from "./DetailSection";
import { formatRelative } from "../../lib/board";
import { GOAL_OUTCOME_LABELS, GOAL_REVIEW_LABELS } from "../../lib/goals";
import type { GoalView } from "../../lib/goals";

/** Esito, esito della review e data di chiusura di un goal chiuso. */
export default function GoalClosedInfo({ goal, now }: { goal: GoalView; now: number }) {
  return (
    <DetailSection title="Chiusura">
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[13px]">
        {goal.outcome && (
          <>
            <dt className="text-muted-foreground">Esito</dt>
            <dd>{GOAL_OUTCOME_LABELS[goal.outcome] ?? goal.outcome}</dd>
          </>
        )}
        {goal.review && (
          <>
            <dt className="text-muted-foreground">Review</dt>
            <dd>{GOAL_REVIEW_LABELS[goal.review] ?? goal.review}</dd>
          </>
        )}
        {goal.closed_at !== undefined && (
          <>
            <dt className="text-muted-foreground">Chiuso</dt>
            <dd>{formatRelative(goal.closed_at, now)}</dd>
          </>
        )}
      </dl>
    </DetailSection>
  );
}
