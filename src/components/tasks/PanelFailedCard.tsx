import { useState } from "react";
import { Loader2, XCircle } from "lucide-react";
import type { Task } from "../../lib/board";
import { failureReason } from "../../lib/taskPanel";

interface PanelFailedCardProps {
  task: Task;
  /** Riprova: rimette il task in coda. Rigetta con l'errore del comando. */
  onRetry: (taskId: string) => Promise<void>;
  /** Apre il dettaglio del task nel Tasks Manager. */
  onOpen: (taskId: string) => void;
}

/** Card di un task fallito: motivo su una riga e "Riprova". */
export default function PanelFailedCard({ task, onRetry, onOpen }: PanelFailedCardProps) {
  const [retrying, setRetrying] = useState(false);
  const reason = failureReason(task);
  const retry = async () => {
    setRetrying(true);
    try {
      await onRetry(task.id);
    } finally {
      setRetrying(false);
    }
  };
  return (
    <div className="flex min-w-0 items-start gap-2.5 rounded-lg border border-destructive/35 px-2.5 py-[9px]">
      <XCircle className="mt-px h-[15px] w-[15px] flex-shrink-0 text-destructive" aria-hidden="true" />
      <button
        type="button"
        onClick={() => onOpen(task.id)}
        title={reason ? `${task.title}\n${reason}` : task.title}
        className="flex min-w-0 flex-1 flex-col gap-[3px] rounded text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <span className="line-clamp-2 break-words text-[13px] font-medium">{task.title}</span>
        {reason && <span className="truncate text-[11px] text-muted-foreground">{reason}</span>}
      </button>
      <button
        type="button"
        disabled={retrying}
        onClick={() => void retry()}
        className="flex h-[26px] flex-shrink-0 items-center gap-1 rounded-md border border-border-strong px-2.5 text-xs transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
      >
        {retrying && <Loader2 className="h-3 w-3 animate-spin motion-reduce:animate-none" aria-hidden="true" />}
        Riprova
      </button>
    </div>
  );
}
