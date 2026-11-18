import DetailSection from "./DetailSection";
import { blockedByLabel, failReasonLabel } from "../../lib/board";
import type { Task } from "../../lib/board";

interface Props {
  task: Task;
  /** Tutti i task della board: per dare il titolo ai task che bloccano. */
  tasks: Task[];
}

/** Perché un task è fallito ("Fermato dall'utente") o da chi è bloccato ("bloccato da X"). */
export default function DetailIssue({ task, tasks }: Props) {
  const failReason = failReasonLabel(task);
  if (failReason) {
    return (
      <DetailSection title="Motivo">
        <p className="text-[13px] text-destructive">{failReason}</p>
      </DetailSection>
    );
  }
  const blocked = blockedByLabel(task, tasks);
  if (!blocked) return null;
  const names = task.blocked_by.map((id) => tasks.find((t) => t.id === id)?.title ?? id);
  return (
    <DetailSection title="Bloccato">
      <p className="break-words text-[13px] text-destructive">
        {blocked}: non parte finché non si riprova o si segna come fatto ({names.join(", ")}).
      </p>
    </DetailSection>
  );
}
