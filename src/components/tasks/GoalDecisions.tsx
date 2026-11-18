import DetailSection from "./DetailSection";
import GoalNoteForm from "./GoalNoteForm";
import { formatRelative } from "../../lib/board";
import type { BoardView } from "../../lib/board";
import { decisionSourceLabel } from "../../lib/goalDetail";
import type { GoalView } from "../../lib/goals";

interface Props {
  workspaceId: string;
  goal: GoalView;
  open: boolean;
  now: number;
  onBoard: (board: BoardView) => void;
}

/** Registro delle decisioni (dalla più vecchia) e, se il goal è aperto, il campo per aggiungere una nota. */
export default function GoalDecisions({ workspaceId, goal, open, now, onBoard }: Props) {
  return (
    <DetailSection title="Decisioni">
      {goal.decisions.length === 0 ? (
        <p className="text-xs text-muted-foreground">Nessuna decisione registrata.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {goal.decisions.map((d, i) => (
            <li key={`${d.at}-${i}`} className="flex flex-col gap-0.5 rounded-md border bg-surface px-3 py-2">
              <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <span className="font-medium">{decisionSourceLabel(d.source)}</span>
                <span aria-hidden="true">·</span>
                <span>{formatRelative(d.at, now)}</span>
              </span>
              <p className="whitespace-pre-line break-words text-[13px] leading-relaxed">{d.text}</p>
            </li>
          ))}
        </ul>
      )}
      {open && <GoalNoteForm workspaceId={workspaceId} goalId={goal.id} onBoard={onBoard} />}
    </DetailSection>
  );
}
