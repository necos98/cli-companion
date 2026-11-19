import { useId, useState } from "react";
import { ChevronDown, ChevronRight, Lightbulb } from "lucide-react";
import type { BoardView } from "../../lib/board";
import type { FollowUpEntry } from "../../lib/followUps";
import type { GoalView } from "../../lib/goals";
import FollowUpRow from "./FollowUpRow";

interface Props {
  workspaceId: string;
  goal: GoalView;
  /** I follow-up aperti del goal. */
  entries: FollowUpEntry[];
  onBoard: (board: BoardView) => void;
}

/** I follow-up aperti di un goal chiuso da poco, comprimibili: per ognuno si sceglie se scartarlo o mandarlo all'Intake. */
export default function FollowUpsCard({ workspaceId, goal, entries, onBoard }: Props) {
  const [expanded, setExpanded] = useState(true);
  const listId = useId();
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-lg border border-primary/25 px-2.5 py-[9px]">
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={listId}
        onClick={() => setExpanded((e) => !e)}
        title={goal.title}
        className="flex min-w-0 items-center gap-2 rounded text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <Lightbulb className="h-[15px] w-[15px] flex-shrink-0 text-primary" aria-hidden="true" />
        <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
          <span className="line-clamp-2 break-words text-[13px] font-medium">{goal.title}</span>
          <span className="text-[11px] text-muted-foreground">
            {entries.length === 1 ? "1 follow-up aperto" : `${entries.length} follow-up aperti`}
          </span>
        </span>
        <Chevron className="h-3.5 w-3.5 flex-shrink-0 text-muted-foreground" aria-hidden="true" />
      </button>
      {expanded && (
        <ul id={listId} className="flex min-w-0 flex-col gap-2">
          {entries.map((entry) => (
            <li key={entry.index} className="min-w-0">
              <FollowUpRow workspaceId={workspaceId} goal={goal} entry={entry} onBoard={onBoard} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
