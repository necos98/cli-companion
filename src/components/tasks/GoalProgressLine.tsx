import { goalPhaseLabel, goalProgressLabel, goalRoundLabel } from "../../lib/goals";
import type { GoalView } from "../../lib/goals";

/** "In lavorazione · giro 2 · 3/5 task": fase, giro e avanzamento di un goal. */
export default function GoalProgressLine({ goal }: { goal: GoalView }) {
  const parts = [goalPhaseLabel(goal), goalRoundLabel(goal), `${goalProgressLabel(goal)} task`];
  return <span>{parts.filter(Boolean).join(" · ")}</span>;
}
