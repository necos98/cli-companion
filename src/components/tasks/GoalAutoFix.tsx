import DetailSection from "./DetailSection";
import { useCommand } from "./useDetailCommand";
import type { BoardView } from "../../lib/board";
import { autoFixChoices, autoFixUsageLabel } from "../../lib/autoFix";
import type { GoalView } from "../../lib/goals";
import { api } from "../../lib/tauri";
import { selectClass } from "../settings/shared";

interface Props {
  workspaceId: string;
  goal: GoalView;
  onBoard: (board: BoardView) => void;
}

/** Correzioni automatiche di un goal aperto: quanti giri di correzione entrano in coda da soli e quanti ne sono usati. */
export default function GoalAutoFix({ workspaceId, goal, onBoard }: Props) {
  const { run, busy, error } = useCommand(onBoard);
  const usage = autoFixUsageLabel(goal);

  return (
    <DetailSection title="Correzioni automatiche">
      <label className="flex flex-col gap-1 text-xs text-text-secondary">
        Giri accettati da soli
        <select
          className={selectClass}
          value={goal.auto_fix.max_rounds}
          disabled={busy}
          onChange={(e) => void run(() => api.goalSetAutoFix(workspaceId, goal.id, Number(e.target.value)))}
        >
          {autoFixChoices().map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
        </select>
      </label>
      <p className="text-xs text-muted-foreground">
        Le correzioni proposte dalla review entrano in coda da sole fino al tetto; oltre, il goal aspetta te.
        {usage && ` Giri automatici usati: ${usage}.`}
      </p>
      {error && (
        <p role="alert" className="break-words text-xs text-destructive">
          {error}
        </p>
      )}
    </DetailSection>
  );
}
