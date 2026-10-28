import DetailSection from "./DetailSection";
import E2eResult from "./E2eResult";
import OpenE2eReportButton from "./OpenE2eReportButton";
import { E2E_SECTION_TITLE, taskE2eRun } from "../../lib/e2e";
import type { Task } from "../../lib/board";

/** Esito degli E2E verificati dall'app: ultima esecuzione del task (o quella del report), con "Apri report". */
export default function DetailE2e({ workspaceId, task }: { workspaceId: string; task: Task }) {
  const run = taskE2eRun(task);
  if (!run) return null;
  return (
    <DetailSection title={E2E_SECTION_TITLE}>
      <div className="flex min-w-0 flex-col gap-2 rounded-lg border bg-surface p-3">
        <E2eResult run={run} />
        <OpenE2eReportButton workspaceId={workspaceId} taskId={task.id} disabled={run.status === "running"} />
      </div>
    </DetailSection>
  );
}
