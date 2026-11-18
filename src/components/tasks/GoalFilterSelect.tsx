import { selectClass } from "../settings/shared";
import type { GoalChoice, GoalFilter } from "../../lib/goals";
import { cn } from "../../lib/utils";

const ALL_GOALS = "Tutti i goal";

interface Props {
  choices: GoalChoice[];
  /** Filtro attivo (già ricondotto a un goal esistente); null = tutti. */
  value: GoalFilter;
  onChange: (filter: GoalFilter) => void;
}

/** Selettore del filtro per goal di Kanban e Lista. */
export default function GoalFilterSelect({ choices, value, onChange }: Props) {
  return (
    <select
      aria-label="Goal"
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value === "" ? null : e.target.value)}
      className={cn(selectClass, "h-8 w-auto max-w-[220px] bg-base text-[13px]", value !== null && "border-primary")}
    >
      <option value="">{ALL_GOALS}</option>
      {choices.map((c) => (
        <option key={c.value} value={c.value}>
          {c.label}
        </option>
      ))}
    </select>
  );
}
