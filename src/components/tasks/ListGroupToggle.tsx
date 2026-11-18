import { cn } from "../../lib/utils";

interface Props {
  on: boolean;
  onChange: (on: boolean) => void;
}

/** Interruttore "Raggruppa per goal" della barra della Lista. */
export default function ListGroupToggle({ on, onChange }: Props) {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={() => onChange(!on)}
      className={cn(
        "flex h-[30px] items-center rounded-full border px-2.5 text-[13px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        on
          ? "border-primary bg-surface-2 text-foreground"
          : "border-border-strong bg-transparent text-muted-foreground hover:text-foreground",
      )}
    >
      Raggruppa per goal
    </button>
  );
}
