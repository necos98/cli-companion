import { AlertTriangle, MessageCircleQuestion } from "lucide-react";
import type { Task } from "../../lib/board";
import { cn } from "@/lib/utils";

interface AttentionCardProps {
  task: Task;
  /** Domanda del worker (blu, decide l'utente) invece di semplice attenzione (arancione). */
  question: boolean;
  onClick: () => void;
}

/** Card compatta di un task in corso che aspetta l'utente: un solo pulsante. */
export default function AttentionCard({ task, question, onClick }: AttentionCardProps) {
  const Icon = question ? MessageCircleQuestion : AlertTriangle;
  return (
    <button
      type="button"
      onClick={onClick}
      title={task.title}
      className={cn(
        "flex w-full min-w-0 items-start gap-2.5 rounded-lg border bg-surface px-2.5 py-[9px] text-left transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        question ? "border-primary/45" : "border-status-attention/45",
      )}
    >
      <Icon
        className={cn("mt-px h-[15px] w-[15px] flex-shrink-0", question ? "text-primary" : "text-status-attention")}
        aria-hidden="true"
      />
      <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
        <span className="line-clamp-2 break-words text-[13px] font-medium">{task.title}</span>
        <span className={cn("truncate text-[11px]", question ? "text-primary" : "text-status-attention")}>
          {question ? "Risposta attesa" : "Richiede attenzione"}
        </span>
      </span>
    </button>
  );
}
