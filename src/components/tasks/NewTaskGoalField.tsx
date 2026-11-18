import { selectClass } from "../settings/shared";
import type { GoalView } from "../../lib/goals";

/** Valore "nessun goal": il task resta sciolto. */
export const NO_GOAL_VALUE = "";

interface Props {
  id: string;
  /** Goal aperti fra cui scegliere. */
  goals: GoalView[];
  value: string;
  onChange: (goalId: string) => void;
  disabled: boolean;
}

/** Scelta facoltativa del goal in cui entra il nuovo task. */
export default function NewTaskGoalField({ id, goals, value, onChange, disabled }: Props) {
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-xs font-medium">
        Goal
      </label>
      <select
        id={id}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        className={selectClass}
      >
        <option value={NO_GOAL_VALUE}>Nessun goal</option>
        {goals.map((g) => (
          <option key={g.id} value={g.id}>
            {g.title}
          </option>
        ))}
      </select>
      <p className="text-[11px] text-muted-foreground">
        Un task di un goal dipende solo da task dello stesso goal; uno senza goal non dipende da quelli di un goal
        aperto.
      </p>
    </div>
  );
}
