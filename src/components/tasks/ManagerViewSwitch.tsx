import { MANAGER_VIEWS } from "../../lib/taskViews";
import type { ManagerView } from "../../lib/taskViews";
import { cn } from "../../lib/utils";

interface Props {
  view: ManagerView;
  onView: (view: ManagerView) => void;
}

/** Selettore delle viste: Kanban | Lista | Dipendenze. */
export default function ManagerViewSwitch({ view, onView }: Props) {
  return (
    <nav aria-label="Viste" className="flex gap-0.5 rounded-lg border bg-surface p-[3px]">
      {MANAGER_VIEWS.map((v) => (
        <button
          key={v.id}
          type="button"
          aria-pressed={view === v.id}
          onClick={() => onView(v.id)}
          className={cn(
            "h-7 rounded-md px-3 text-[13px] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            view === v.id
              ? "bg-surface-2 font-medium text-foreground"
              : "text-text-secondary hover:text-foreground",
          )}
        >
          {v.label}
        </button>
      ))}
    </nav>
  );
}
