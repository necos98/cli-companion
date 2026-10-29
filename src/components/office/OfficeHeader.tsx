import { ArrowLeft, Building2 } from "lucide-react";
import { Button } from "../ui/button";

interface Props {
  botCount: number;
  onClose: () => void;
}

/** Barra dell'Ufficio: titolo, numero di bot e ritorno ai terminali. */
export default function OfficeHeader({ botCount, onClose }: Props) {
  return (
    <header className="flex flex-shrink-0 flex-wrap items-center gap-x-3 gap-y-2 border-b bg-base px-4 py-2.5">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onClose}
        className="h-8 gap-1.5 bg-transparent text-[13px] text-text-secondary"
      >
        <ArrowLeft className="!size-3.5" /> Torna ai terminali
      </Button>
      <Building2 className="size-4 flex-shrink-0 text-muted-foreground" aria-hidden="true" />
      <div className="flex min-w-0 flex-col">
        <h1 className="text-[15px] font-semibold leading-tight">Ufficio</h1>
        <span className="truncate text-[11px] text-muted-foreground">{botCount} bot</span>
      </div>
    </header>
  );
}
