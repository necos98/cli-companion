import DetailSection from "./DetailSection";
import FollowUpRow from "./FollowUpRow";
import type { BoardView } from "../../lib/board";
import { followUpEntries } from "../../lib/followUps";
import type { GoalView } from "../../lib/goals";

interface Props {
  workspaceId: string;
  goal: GoalView;
  onBoard: (board: BoardView) => void;
}

/** Tutti i follow-up di un goal chiuso con il loro stato; quelli aperti si scartano o si mandano all'Intake. Assente se non ce ne sono. */
export default function GoalFollowUps({ workspaceId, goal, onBoard }: Props) {
  const entries = followUpEntries(goal);
  if (entries.length === 0) return null;
  return (
    <DetailSection title="Follow-up">
      <ul className="flex flex-col gap-2">
        {entries.map((entry) => (
          <li key={entry.index} className="min-w-0">
            <FollowUpRow workspaceId={workspaceId} goal={goal} entry={entry} showState onBoard={onBoard} />
          </li>
        ))}
      </ul>
    </DetailSection>
  );
}
