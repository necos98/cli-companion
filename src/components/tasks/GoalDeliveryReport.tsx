import { ExternalLink } from "lucide-react";
import DetailSection from "./DetailSection";
import { openExternal } from "../../lib/openExternal";
import { prLinkLabel } from "../../lib/goalDelivery";
import type { GoalView } from "../../lib/goals";

/** Cosa ha consegnato il task di consegna di un goal chiuso: riepilogo, commit finali e link delle PR. */
export default function GoalDeliveryReport({ goal }: { goal: GoalView }) {
  const report = goal.report;
  if (!report) return null;
  return (
    <DetailSection title="Consegnato">
      {report.summary && (
        <p className="whitespace-pre-line break-words text-[13px] leading-relaxed text-text-secondary">
          {report.summary}
        </p>
      )}
      {report.commits.length > 0 && (
        <ul aria-label="Commit" className="flex flex-col gap-0.5 font-mono text-xs text-text-secondary">
          {report.commits.map((c) => (
            <li key={c} className="break-all">
              <span className="font-sans text-muted-foreground">commit </span>
              {c}
            </li>
          ))}
        </ul>
      )}
      {report.pr_urls.length > 0 && (
        <ul aria-label="Pull request" className="flex flex-wrap gap-x-3 gap-y-1 text-xs">
          {report.pr_urls.map((url) => (
            <li key={url}>
              <button
                type="button"
                onClick={() => openExternal(url)}
                title={url}
                className="flex items-center gap-1 rounded font-mono text-primary hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                {prLinkLabel(url)} <ExternalLink className="h-3 w-3" aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </DetailSection>
  );
}
