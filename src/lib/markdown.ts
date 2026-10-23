// Markdown minimale per la specifica dei task: titoli, paragrafi, liste (anche
// annidate), citazioni, blocchi di codice, **grassetto**, *corsivo*, `codice`,
// link. Produce un albero di dati (mai HTML): il rendering costruisce elementi
// React, quindi niente `dangerouslySetInnerHTML`. Funzioni pure a zero
// dipendenze, importabili sia dall'app sia dai test `node --test`.

export type MdInline =
  | { t: "text"; text: string }
  | { t: "code"; text: string }
  | { t: "strong"; children: MdInline[] }
  | { t: "em"; children: MdInline[] }
  | { t: "link"; href: string; children: MdInline[] };

export interface MdListItem {
  children: MdInline[];
  /** Liste annidate sotto questo elemento. */
  sub: MdList[];
}

export interface MdList {
  t: "list";
  ordered: boolean;
  items: MdListItem[];
}

export type MdBlock =
  | { t: "heading"; level: number; children: MdInline[] }
  | { t: "paragraph"; children: MdInline[] }
  | { t: "quote"; children: MdInline[] }
  | { t: "code"; lang: string; text: string }
  | { t: "rule" }
  | MdList;

/** Solo http(s) e mailto: qualunque altro schema (javascript:, file:, data:…) non è un link. */
export function safeHref(raw: string): string | null {
  const u = raw.trim();
  return /^(https?:\/\/|mailto:)/i.test(u) ? u : null;
}

// ---------------------------------------------------------------- inline

const PUNCT = /[\\`*_{}[\]()#+\-.!~|<>]/;
const isSpace = (c: string | undefined) => c === undefined || /\s/.test(c);
const isWord = (c: string | undefined) => c !== undefined && /[\p{L}\p{N}]/u.test(c);

/** Indice del delimitatore di chiusura `marker` dopo `from`, o -1. */
function findClose(s: string, marker: string, from: number): number {
  let j = s.indexOf(marker, from);
  while (j !== -1) {
    const single = marker.length === 1;
    const prev = s[j - 1];
    const next = s[j + marker.length];
    const ok =
      j > from &&
      !isSpace(prev) &&
      // `*a**b*`: un delimitatore singolo non fa parte di uno doppio.
      !(single && (prev === marker || next === marker));
    if (ok) return j;
    j = s.indexOf(marker, j + 1);
  }
  return -1;
}

/** Indice della `)` che chiude la `(` in `open`, tenendo conto delle parentesi annidate. */
function findParen(s: string, open: number): number {
  let depth = 0;
  for (let i = open; i < s.length; i++) {
    if (s[i] === "(") depth++;
    else if (s[i] === ")" && --depth === 0) return i;
    else if (s[i] === "\n") return -1;
  }
  return -1;
}

const AUTOLINK = /^https?:\/\/[^\s<>]+/i;

/** Testo inline → nodi. Mai HTML: i caratteri speciali restano testo. */
export function parseInline(s: string): MdInline[] {
  const out: MdInline[] = [];
  let buf = "";
  const flush = () => {
    if (buf !== "") out.push({ t: "text", text: buf });
    buf = "";
  };

  let i = 0;
  while (i < s.length) {
    const c = s[i];

    if (c === "\\" && i + 1 < s.length && PUNCT.test(s[i + 1])) {
      buf += s[i + 1];
      i += 2;
      continue;
    }

    if (c === "`") {
      let n = 1;
      while (s[i + n] === "`") n++;
      const fence = "`".repeat(n);
      const end = s.indexOf(fence, i + n);
      if (end !== -1) {
        flush();
        let text = s.slice(i + n, end);
        if (text.length > 1 && text.startsWith(" ") && text.endsWith(" ")) text = text.slice(1, -1);
        out.push({ t: "code", text });
        i = end + n;
        continue;
      }
      buf += fence;
      i += n;
      continue;
    }

    if (c === "[") {
      const close = s.indexOf("]", i + 1);
      if (close !== -1 && s[close + 1] === "(") {
        const end = findParen(s, close + 1);
        if (end !== -1) {
          let url = s.slice(close + 2, end).trim().split(/\s+/)[0] ?? "";
          if (url.startsWith("<") && url.endsWith(">")) url = url.slice(1, -1);
          const children = parseInline(s.slice(i + 1, close));
          const href = safeHref(url);
          flush();
          if (href) out.push({ t: "link", href, children });
          else out.push(...children);
          i = end + 1;
          continue;
        }
      }
    }

    if (c === "*" || c === "_") {
      const double = s[i + 1] === c;
      const marker = double ? c + c : c;
      const start = i + marker.length;
      // `_` vale come corsivo solo ai bordi di una parola (snake_case resta testo).
      const boundaryOk = c === "*" || !isWord(s[i - 1]);
      if (boundaryOk && !isSpace(s[start])) {
        const end = findClose(s, marker, start);
        if (end !== -1 && (c === "*" || !isWord(s[end + marker.length]))) {
          flush();
          const children = parseInline(s.slice(start, end));
          out.push({ t: double ? "strong" : "em", children });
          i = end + marker.length;
          continue;
        }
      }
      buf += marker;
      i += marker.length;
      continue;
    }

    if ((c === "h" || c === "H") && !isWord(s[i - 1])) {
      const m = AUTOLINK.exec(s.slice(i));
      if (m) {
        // La punteggiatura finale non fa parte dell'indirizzo.
        const url = m[0].replace(/[.,;:!?)\]'"]+$/, "");
        const href = safeHref(url);
        if (href) {
          flush();
          out.push({ t: "link", href, children: [{ t: "text", text: url }] });
          i += url.length;
          continue;
        }
      }
    }

    buf += c;
    i++;
  }
  flush();
  return out;
}

// ---------------------------------------------------------------- blocchi

const FENCE = /^\s{0,3}(```+|~~~+)\s*([^\s`]*)/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/;
const LIST_ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const QUOTE = /^\s{0,3}>\s?(.*)$/;

interface RawItem {
  indent: number;
  ordered: boolean;
  text: string;
}

/** Liste a partire da `items` (già in sequenza), annidate per indentazione. */
function buildLists(items: RawItem[]): MdList[] {
  const lists: MdList[] = [];
  // Pila di (indentazione, lista) aperte.
  const stack: { indent: number; list: MdList }[] = [];
  for (const it of items) {
    while (stack.length > 0 && it.indent < stack[stack.length - 1].indent) stack.pop();
    let top = stack[stack.length - 1];
    if (!top || it.indent > top.indent) {
      const list: MdList = { t: "list", ordered: it.ordered, items: [] };
      const parent = top?.list.items[top.list.items.length - 1];
      if (parent) parent.sub.push(list);
      else lists.push(list);
      top = { indent: it.indent, list };
      stack.push(top);
    }
    top.list.items.push({ children: parseInline(it.text), sub: [] });
  }
  return lists;
}

/** Markdown → blocchi. Tollerante: l'input malformato diventa paragrafi di testo. */
export function parseMarkdown(src: string): MdBlock[] {
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  const blocks: MdBlock[] = [];
  let para: string[] = [];
  let listItems: RawItem[] = [];
  let quote: string[] = [];

  const flushPara = () => {
    if (para.length > 0) blocks.push({ t: "paragraph", children: parseInline(para.join("\n")) });
    para = [];
  };
  const flushList = () => {
    if (listItems.length > 0) blocks.push(...buildLists(listItems));
    listItems = [];
  };
  const flushQuote = () => {
    if (quote.length > 0) blocks.push({ t: "quote", children: parseInline(quote.join("\n")) });
    quote = [];
  };
  const flushAll = () => {
    flushPara();
    flushList();
    flushQuote();
  };

  for (let n = 0; n < lines.length; n++) {
    const line = lines[n];

    const fence = FENCE.exec(line);
    if (fence) {
      flushAll();
      const marker = fence[1];
      const body: string[] = [];
      n++;
      while (n < lines.length && !new RegExp(`^\\s{0,3}${marker[0]}{${marker.length},}\\s*$`).test(lines[n])) {
        body.push(lines[n]);
        n++;
      }
      blocks.push({ t: "code", lang: fence[2], text: body.join("\n") });
      continue;
    }

    if (line.trim() === "") {
      flushAll();
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      flushAll();
      blocks.push({ t: "heading", level: heading[1].length, children: parseInline(heading[2]) });
      continue;
    }

    if (RULE.test(line)) {
      flushAll();
      blocks.push({ t: "rule" });
      continue;
    }

    const q = QUOTE.exec(line);
    if (q) {
      flushPara();
      flushList();
      quote.push(q[1]);
      continue;
    }

    const li = LIST_ITEM.exec(line);
    if (li) {
      flushPara();
      flushQuote();
      listItems.push({
        indent: li[1].replace(/\t/g, "    ").length,
        ordered: /^\d/.test(li[2]),
        text: li[3],
      });
      continue;
    }

    // Riga indentata sotto una voce di lista: ne continua il testo.
    if (listItems.length > 0 && /^\s+/.test(line)) {
      listItems[listItems.length - 1].text += `\n${line.trim()}`;
      continue;
    }

    flushList();
    flushQuote();
    para.push(line);
  }
  flushAll();
  return blocks;
}
