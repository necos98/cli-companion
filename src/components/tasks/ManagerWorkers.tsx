import { Minus, Pause, Play, Plus } from "lucide-react";
import { MAX_PARALLEL_MAX, MAX_PARALLEL_MIN, SERIAL_WORKERS_TEXT, isParallel } from "../../lib/board";
import type { BoardView } from "../../lib/board";
import { Button } from "../ui/button";

interface Props {
  board: BoardView | null;
  busy: boolean;
  onMaxParallel: (n: number) => void;
  onTogglePause: () => void;
}

const stepClass =
  "flex h-6 w-6 items-center justify-center rounded text-text-secondary hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-40";

/** Stepper dei worker (solo in modalità parallela) e Pausa / Riprendi della coda. */
export default function ManagerWorkers({ board, busy, onMaxParallel, onTogglePause }: Props) {
  const parallel = board !== null && isParallel(board.delivery);
  const value = board?.max_parallel ?? MAX_PARALLEL_MIN;
  return (
    <>
      <div
        role="group"
        aria-label="Worker in parallelo"
        className="flex h-8 items-center gap-1 rounded-md border border-border-strong pl-2.5 pr-1"
      >
        <span className="mr-1 text-xs text-text-secondary">Worker</span>
        {parallel ? (
          <>
            <button
              type="button"
              aria-label="Meno worker"
              disabled={busy || value <= MAX_PARALLEL_MIN}
              onClick={() => onMaxParallel(value - 1)}
              className={stepClass}
            >
              <Minus className="h-3.5 w-3.5" />
            </button>
            <span className="w-5 text-center font-mono text-[13px]" aria-live="polite">
              {value}
            </span>
            <button
              type="button"
              aria-label="Più worker"
              disabled={busy || value >= MAX_PARALLEL_MAX}
              onClick={() => onMaxParallel(value + 1)}
              className={stepClass}
            >
              <Plus className="h-3.5 w-3.5" />
            </button>
          </>
        ) : (
          <span className="pr-1.5 text-xs text-muted-foreground">{SERIAL_WORKERS_TEXT}</span>
        )}
      </div>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={busy || board === null}
        onClick={onTogglePause}
        className="h-8 gap-1.5 bg-transparent text-[13px] text-text-secondary"
      >
        {board?.paused ? <Play className="!size-3.5" /> : <Pause className="!size-3.5" />}
        {board?.paused ? "Riprendi" : "Pausa"}
      </Button>
    </>
  );
}
