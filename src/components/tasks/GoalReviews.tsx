import DetailSection from "./DetailSection";
import { VerdictBadge } from "./ReviewBadges";
import { displayStatus, statusLabel } from "../../lib/board";
import type { Task } from "../../lib/board";
import { isReview, verdictBadge } from "../../lib/review";

interface Props {
  /** Task del goal, nell'ordine della board. */
  tasks: Task[];
  onSelectTask: (taskId: string) => void;
}

/** Esito delle review del goal: il verdetto di ognuna, oppure lo stato finché non conclude. */
export default function GoalReviews({ tasks, onSelectTask }: Props) {
  const reviews = tasks.filter(isReview);
  if (reviews.length === 0) return null;
  return (
    <DetailSection title="Esito delle review">
      <ul className="flex flex-col gap-1">
        {reviews.map((t) => (
          <li key={t.id}>
            <button
              type="button"
              onClick={() => onSelectTask(t.id)}
              className="flex w-full min-w-0 items-center gap-2 rounded-md border border-border-strong px-2.5 py-1.5 text-left text-xs hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <span className="min-w-0 flex-1 truncate">{t.title}</span>
              {verdictBadge(t) ? (
                <VerdictBadge task={t} className="text-[11px]" />
              ) : (
                <span className="flex-shrink-0 text-muted-foreground">{statusLabel(displayStatus(t))}</span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </DetailSection>
  );
}
