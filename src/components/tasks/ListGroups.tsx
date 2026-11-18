import type { ReactNode } from "react";
import GoalProgressLine from "./GoalProgressLine";
import type { GoalGroup } from "../../lib/goals";
import type { Task } from "../../lib/board";

interface Props {
  groups: GoalGroup<Task>[];
  renderRow: (task: Task) => ReactNode;
  onOpenGoal: (goalId: string) => void;
}

const HEADER_CLASS =
  "flex min-w-[700px] items-baseline gap-3 border-b bg-surface px-4 py-2 text-left text-[13px] font-semibold";

/** Intestazione di un gruppo: un bottone che apre il dettaglio se è un goal, altrimenti solo il titolo. */
function GroupHeader({ group, onOpenGoal }: { group: GoalGroup<Task>; onOpenGoal: (goalId: string) => void }) {
  const { goal } = group;
  if (!goal) {
    return (
      <h3 className={HEADER_CLASS}>
        <span className="truncate">{group.title}</span>
      </h3>
    );
  }
  return (
    <button
      type="button"
      aria-label={`Apri il goal ${group.title}`}
      onClick={() => onOpenGoal(goal.id)}
      className={`${HEADER_CLASS} w-full hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring`}
    >
      <span className="truncate">{group.title}</span>
      <span className="flex-shrink-0 text-xs font-normal text-muted-foreground">
        <GoalProgressLine goal={goal} />
      </span>
    </button>
  );
}

/** Righe della Lista suddivise per goal: intestazione (titolo, fase e avanzamento) e, sotto, i task del gruppo. */
export default function ListGroups({ groups, renderRow, onOpenGoal }: Props) {
  return (
    <>
      {groups.map((g) => (
        <section key={g.key} aria-label={`Goal: ${g.title}`}>
          <GroupHeader group={g} onOpenGoal={onOpenGoal} />
          {g.tasks.map(renderRow)}
        </section>
      ))}
    </>
  );
}
