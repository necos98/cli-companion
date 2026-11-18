import { useId, useState } from "react";
import type { FormEvent } from "react";
import { useCommand } from "./useDetailCommand";
import { Button } from "../ui/button";
import type { BoardView } from "../../lib/board";
import { GOAL_NOTE_MAX, goalNoteError } from "../../lib/goalDetail";
import { api } from "../../lib/tauri";

interface Props {
  workspaceId: string;
  goalId: string;
  onBoard: (board: BoardView) => void;
}

/** "Aggiungi nota": una voce dell'utente nel registro delle decisioni del goal (solo goal aperti). */
export default function GoalNoteForm({ workspaceId, goalId, onBoard }: Props) {
  const id = useId();
  const [text, setText] = useState("");
  const [invalid, setInvalid] = useState<string | null>(null);
  const { run, busy, error, clearError } = useCommand(onBoard);
  const length = [...text.trim()].length;
  const problem = invalid ?? error;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const found = goalNoteError(text);
    setInvalid(found ?? null);
    if (found) return;
    if (await run(() => api.goalAddNote(workspaceId, goalId, text.trim()))) setText("");
  };

  return (
    <form onSubmit={submit} noValidate className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-xs font-medium">
        Aggiungi nota
      </label>
      <textarea
        id={id}
        value={text}
        rows={3}
        disabled={busy}
        aria-invalid={problem ? true : undefined}
        aria-describedby={problem ? `${id}-error` : undefined}
        onChange={(e) => {
          setText(e.target.value);
          setInvalid(null);
          clearError();
        }}
        className="w-full resize-y rounded-md border border-input bg-transparent px-3 py-2 text-[13px] leading-relaxed outline-none placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
      />
      {problem && (
        <p id={`${id}-error`} role="alert" className="break-words text-[11px] text-destructive">
          {problem}
        </p>
      )}
      <div className="flex items-center justify-between gap-2">
        <span className="font-mono text-[11px] text-muted-foreground">
          {length}/{GOAL_NOTE_MAX}
        </span>
        <Button type="submit" size="sm" variant="outline" disabled={busy} className="bg-transparent">
          {busy ? "Salvataggio…" : "Salva nota"}
        </Button>
      </div>
    </form>
  );
}
