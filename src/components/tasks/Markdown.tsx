import { Fragment, useMemo } from "react";
import type { ReactNode } from "react";
import { parseMarkdown } from "../../lib/markdown";
import type { MdBlock, MdInline, MdList } from "../../lib/markdown";
import { openExternal } from "../../lib/openExternal";

function Inline({ nodes }: { nodes: MdInline[] }): ReactNode {
  return (
    <>
      {nodes.map((n, i) => {
        switch (n.t) {
          case "text":
            return <Fragment key={i}>{n.text}</Fragment>;
          case "code":
            return (
              <code key={i} className="rounded bg-surface-2 px-1 py-px font-mono text-[12px] text-foreground">
                {n.text}
              </code>
            );
          case "strong":
            return (
              <strong key={i} className="font-semibold text-foreground">
                <Inline nodes={n.children} />
              </strong>
            );
          case "em":
            return (
              <em key={i}>
                <Inline nodes={n.children} />
              </em>
            );
          case "link":
            return (
              <a
                key={i}
                href={n.href}
                title={n.href}
                // Si apre nel browser di sistema, mai dentro la finestra dell'app.
                onClick={(e) => {
                  e.preventDefault();
                  openExternal(n.href);
                }}
                className="break-words text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <Inline nodes={n.children} />
              </a>
            );
        }
      })}
    </>
  );
}

function List({ list }: { list: MdList }) {
  const Tag = list.ordered ? "ol" : "ul";
  return (
    <Tag className={`flex flex-col gap-1 pl-5 ${list.ordered ? "list-decimal" : "list-disc"}`}>
      {list.items.map((item, i) => (
        <li key={i} className="whitespace-pre-line pl-0.5 marker:text-muted-foreground">
          <Inline nodes={item.children} />
          {item.sub.map((s, j) => (
            <div key={j} className="mt-1 whitespace-normal">
              <List list={s} />
            </div>
          ))}
        </li>
      ))}
    </Tag>
  );
}

const HEADING_CLASS: Record<number, string> = {
  1: "text-[15px] font-semibold text-foreground",
  2: "text-[14px] font-semibold text-foreground",
  3: "text-[13px] font-semibold text-foreground",
};

function Block({ block }: { block: MdBlock }) {
  switch (block.t) {
    case "heading":
      return (
        <p role="heading" aria-level={block.level + 2} className={`pt-1 ${HEADING_CLASS[block.level] ?? "text-[13px] font-medium text-foreground"}`}>
          <Inline nodes={block.children} />
        </p>
      );
    case "paragraph":
      return (
        <p className="whitespace-pre-line break-words">
          <Inline nodes={block.children} />
        </p>
      );
    case "quote":
      return (
        <blockquote className="whitespace-pre-line border-l-2 border-border-strong pl-3 text-muted-foreground">
          <Inline nodes={block.children} />
        </blockquote>
      );
    case "code":
      return (
        <pre
          tabIndex={0}
          className="overflow-x-auto rounded-md border bg-surface px-3 py-2 font-mono text-[12px] leading-relaxed text-text-secondary"
        >
          <code>{block.text}</code>
        </pre>
      );
    case "rule":
      return <hr className="border-border" />;
    case "list":
      return <List list={block} />;
  }
}

/** Markdown della specifica di un task, reso con elementi React (niente HTML grezzo). */
export default function Markdown({ source }: { source: string }) {
  const blocks = useMemo(() => parseMarkdown(source), [source]);
  if (blocks.length === 0) return <p className="text-muted-foreground">Nessuna specifica.</p>;
  return (
    <div className="flex min-w-0 flex-col gap-2.5 text-[13px] leading-[1.55] text-text-secondary">
      {blocks.map((b, i) => (
        <Block key={i} block={b} />
      ))}
    </div>
  );
}

