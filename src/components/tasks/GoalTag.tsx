import { Flag } from "lucide-react";
import { cn } from "../../lib/utils";

/** Etichetta col titolo del goal di un task (la bandierina e il testo, non solo il colore). */
export default function GoalTag({ title, className }: { title: string; className?: string }) {
  return (
    <span
      title={`Goal: ${title}`}
      className={cn(
        "inline-flex min-w-0 max-w-full items-center gap-1 rounded border border-border-strong px-1.5 py-px text-muted-foreground",
        className,
      )}
    >
      <Flag className="h-3 w-3 flex-shrink-0" aria-hidden="true" />
      <span className="sr-only">Goal: </span>
      <span className="truncate">{title}</span>
    </span>
  );
}
