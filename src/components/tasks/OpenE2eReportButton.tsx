import { useState } from "react";
import { FileText } from "lucide-react";
import { Button } from "../ui/button";
import { api } from "../../lib/tauri";

interface Props {
  workspaceId: string;
  /** Report dell'ultima esecuzione di questo task; assente = quella manuale del repository. */
  taskId?: string;
  repo?: string;
  /** L'esecuzione e' in corso: il report non c'e' ancora. */
  disabled?: boolean;
}

/** "Apri report": il backend ricava il file JUnit dell'esecuzione e lo apre con l'applicazione predefinita. */
export default function OpenE2eReportButton({ workspaceId, taskId, repo, disabled = false }: Props) {
  const [error, setError] = useState<string | null>(null);
  const open = () => {
    setError(null);
    api.e2eOpenReport(workspaceId, taskId, repo).catch((e) => setError(String(e)));
  };
  return (
    <div className="flex flex-col items-start gap-1">
      <Button type="button" size="sm" variant="outline" disabled={disabled} onClick={open} className="h-7 bg-transparent px-2.5 text-xs">
        <FileText className="mr-1.5 h-3.5 w-3.5" aria-hidden="true" />
        Apri report
      </Button>
      {error && (
        <p role="alert" className="break-words text-[11px] text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
