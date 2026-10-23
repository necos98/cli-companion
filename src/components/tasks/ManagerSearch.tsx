import { Search, X } from "lucide-react";
import type { RefObject } from "react";

interface Props {
  value: string;
  onChange: (value: string) => void;
  inputRef: RefObject<HTMLInputElement>;
}

/** Ricerca sempre visibile: `/` la mette a fuoco (gestito dal manager), Esc la svuota. */
export default function ManagerSearch({ value, onChange, inputRef }: Props) {
  return (
    <label className="flex h-8 min-w-0 max-w-[360px] flex-[1_1_220px] items-center gap-2 rounded-md border border-border-strong bg-pane-area px-2.5 focus-within:ring-1 focus-within:ring-ring">
      <Search className="h-3.5 w-3.5 flex-shrink-0 text-muted-foreground" aria-hidden="true" />
      <span className="sr-only">Cerca nei task</span>
      <input
        ref={inputRef}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key !== "Escape") return;
          e.stopPropagation();
          if (value !== "") onChange("");
          else e.currentTarget.blur();
        }}
        placeholder="Cerca titolo, specifica, esito…"
        className="min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-muted-foreground"
      />
      {value !== "" ? (
        <button
          type="button"
          aria-label="Svuota la ricerca"
          onClick={() => {
            onChange("");
            inputRef.current?.focus();
          }}
          className="flex-shrink-0 rounded text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      ) : (
        <kbd className="flex-shrink-0 rounded border border-border-strong px-1.5 font-mono text-[11px] text-muted-foreground">
          /
        </kbd>
      )}
    </label>
  );
}
