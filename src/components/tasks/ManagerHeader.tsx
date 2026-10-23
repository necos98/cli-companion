import { ArrowLeft, Plus, Sparkles } from "lucide-react";
import type { RefObject } from "react";
import { Button } from "../ui/button";
import ManagerSearch from "./ManagerSearch";
import ManagerViewSwitch from "./ManagerViewSwitch";
import ManagerWorkers from "./ManagerWorkers";
import type { BoardView } from "../../lib/board";
import type { ManagerView } from "../../lib/taskViews";

interface Props {
  workspaceName: string;
  /** Etichetta della consegna ("Branch + PR + merge"); null finché la board non c'è. */
  delivery: string | null;
  board: BoardView | null;
  view: ManagerView;
  onView: (view: ManagerView) => void;
  query: string;
  onQuery: (query: string) => void;
  searchRef: RefObject<HTMLInputElement>;
  busy: boolean;
  onBackToTerminals: () => void;
  onMaxParallel: (n: number) => void;
  onTogglePause: () => void;
  onNewTask: () => void;
  onOpenIntake: () => void;
}

/** Barra del Tasks Manager: indietro, titolo, viste, ricerca, worker, pausa, nuovo task, Task Intake. */
export default function ManagerHeader(p: Props) {
  return (
    <header className="flex flex-shrink-0 flex-wrap items-center gap-x-3 gap-y-2 border-b bg-base px-4 py-2.5">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={p.onBackToTerminals}
        className="h-8 gap-1.5 bg-transparent text-[13px] text-text-secondary"
      >
        <ArrowLeft className="!size-3.5" /> Terminali
      </Button>
      <div className="flex min-w-0 flex-col">
        <h1 className="text-[15px] font-semibold leading-tight">Tasks</h1>
        <span className="truncate text-[11px] text-muted-foreground">
          {p.workspaceName}
          {p.delivery ? ` · consegna: ${p.delivery}` : ""}
        </span>
      </div>
      <div className="ml-2">
        <ManagerViewSwitch view={p.view} onView={p.onView} />
      </div>
      <ManagerSearch value={p.query} onChange={p.onQuery} inputRef={p.searchRef} />
      <div className="ml-auto flex flex-wrap items-center gap-2">
        <ManagerWorkers
          board={p.board}
          busy={p.busy}
          onMaxParallel={p.onMaxParallel}
          onTogglePause={p.onTogglePause}
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={p.onNewTask}
          className="h-8 gap-1.5 bg-transparent text-[13px]"
        >
          <Plus className="!size-3.5" /> Nuovo task
        </Button>
        <Button type="button" size="sm" onClick={p.onOpenIntake} className="h-8 gap-1.5 text-[13px] font-semibold">
          <Sparkles className="!size-3.5" /> Task Intake
        </Button>
      </div>
    </header>
  );
}
