import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";

interface PanelHeaderButtonProps {
  icon: LucideIcon;
  label: string;
  onClick: () => void;
  /** Tooltip; di default l'etichetta. */
  title?: string;
  disabled?: boolean;
  iconClassName?: string;
}

/** Pulsante a sola icona 28x28 dell'intestazione dei pannelli Task. */
export default function PanelHeaderButton({
  icon: Icon,
  label,
  onClick,
  title,
  disabled,
  iconClassName,
}: PanelHeaderButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title ?? label}
      aria-label={label}
      className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-md text-text-secondary transition-colors hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
    >
      <Icon className={cn("h-3.5 w-3.5", iconClassName)} aria-hidden="true" />
    </button>
  );
}
