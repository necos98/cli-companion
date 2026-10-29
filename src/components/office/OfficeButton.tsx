import { Building2 } from "lucide-react";
import { Button } from "../ui/button";
import { cn } from "../../lib/utils";

interface Props {
  /** L'Ufficio e' aperto. */
  shown: boolean;
  onToggle: () => void;
}

/** Pulsante del piede della Sidebar che apre/chiude l'Ufficio. */
export default function OfficeButton({ shown, onToggle }: Props) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      onClick={onToggle}
      aria-label="Ufficio"
      aria-pressed={shown}
      title="Ufficio (Ctrl+Maiusc+O)"
      className={cn(
        "h-8 w-8 flex-shrink-0 text-text-secondary hover:text-foreground",
        shown && "bg-accent text-foreground",
      )}
    >
      <Building2 className="!size-[15px]" />
    </Button>
  );
}
