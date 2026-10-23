import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { parseInline, parseMarkdown, safeHref } from "../src/lib/markdown.ts";

const text = (t) => ({ t: "text", text: t });

describe("markdown: inline", () => {
  it("testo semplice", () => {
    assert.deepEqual(parseInline("ciao mondo"), [text("ciao mondo")]);
  });
  it("grassetto, corsivo e codice", () => {
    assert.deepEqual(parseInline("a **b** *c* `d`"), [
      text("a "),
      { t: "strong", children: [text("b")] },
      text(" "),
      { t: "em", children: [text("c")] },
      text(" "),
      { t: "code", text: "d" },
    ]);
  });
  it("annidamento: corsivo dentro il grassetto", () => {
    assert.deepEqual(parseInline("**a *b* c**"), [
      { t: "strong", children: [text("a "), { t: "em", children: [text("b")] }, text(" c")] },
    ]);
  });
  it("il codice non interpreta il markdown", () => {
    assert.deepEqual(parseInline("`**x**`"), [{ t: "code", text: "**x**" }]);
  });
  it("asterischi isolati e snake_case restano testo", () => {
    assert.deepEqual(parseInline("2 * 3 * 4"), [text("2 * 3 * 4")]);
    assert.deepEqual(parseInline("my_var_name"), [text("my_var_name")]);
    assert.deepEqual(parseInline("**mai chiuso"), [text("**mai chiuso")]);
  });
  it("escape", () => {
    assert.deepEqual(parseInline("\\*non corsivo\\*"), [text("*non corsivo*")]);
  });
  it("link http(s) e mailto", () => {
    assert.deepEqual(parseInline("[doc](https://example.com/a_(b))"), [
      { t: "link", href: "https://example.com/a_(b)", children: [text("doc")] },
    ]);
    assert.equal(parseInline("[m](mailto:a@b.it)")[0].t, "link");
  });
  it("schemi pericolosi: niente link, resta il testo", () => {
    for (const bad of ["javascript:alert(1)", "file:///etc/passwd", "data:text/html,x", "vbscript:x"]) {
      assert.deepEqual(parseInline(`[clic](${bad})`), [text("clic")]);
    }
    assert.equal(safeHref("  JAVASCRIPT:alert(1)"), null);
    assert.equal(safeHref("HTTPS://x.it"), "HTTPS://x.it");
  });
  it("indirizzo nudo diventa link senza la punteggiatura finale", () => {
    assert.deepEqual(parseInline("vedi https://example.com/x."), [
      text("vedi "),
      { t: "link", href: "https://example.com/x", children: [text("https://example.com/x")] },
      text("."),
    ]);
  });
  it("l'html resta testo", () => {
    assert.deepEqual(parseInline("<img src=x onerror=alert(1)>"), [text("<img src=x onerror=alert(1)>")]);
  });
});

describe("markdown: blocchi", () => {
  it("titoli e paragrafi", () => {
    const b = parseMarkdown("# Goal\nfai la cosa\n\n## Context\ntesto");
    assert.deepEqual(
      b.map((x) => x.t),
      ["heading", "paragraph", "heading", "paragraph"],
    );
    assert.equal(b[0].level, 1);
    assert.equal(b[2].level, 2);
  });
  it("le righe consecutive restano nello stesso paragrafo con l'a capo", () => {
    const [p] = parseMarkdown("riga uno\nriga due");
    assert.deepEqual(p.children, [text("riga uno\nriga due")]);
  });
  it("liste puntate e numerate", () => {
    const [a, b] = parseMarkdown("- uno\n- due\n\n1. primo\n2) secondo");
    assert.equal(a.t, "list");
    assert.equal(a.ordered, false);
    assert.equal(a.items.length, 2);
    assert.equal(b.ordered, true);
    assert.equal(b.items.length, 2);
  });
  it("liste annidate per indentazione", () => {
    const [l] = parseMarkdown("- a\n  - a1\n  - a2\n- b");
    assert.equal(l.items.length, 2);
    assert.equal(l.items[0].sub.length, 1);
    assert.equal(l.items[0].sub[0].items.length, 2);
    assert.equal(l.items[1].sub.length, 0);
  });
  it("riga indentata continua la voce di lista", () => {
    const [l] = parseMarkdown("- uno\n  segue");
    assert.deepEqual(l.items[0].children, [text("uno\nsegue")]);
  });
  it("blocchi di codice recintati: il contenuto è letterale", () => {
    const [c] = parseMarkdown("```rust\nfn main() {}\n# non un titolo\n```");
    assert.deepEqual(c, { t: "code", lang: "rust", text: "fn main() {}\n# non un titolo" });
  });
  it("recinto non chiuso: arriva a fine testo", () => {
    const [c] = parseMarkdown("```\na\nb");
    assert.equal(c.text, "a\nb");
  });
  it("citazione e linea orizzontale", () => {
    const b = parseMarkdown("> nota\n> due\n\n---");
    assert.equal(b[0].t, "quote");
    assert.equal(b[1].t, "rule");
  });
  it("CRLF e testo vuoto", () => {
    assert.deepEqual(parseMarkdown(""), []);
    assert.equal(parseMarkdown("a\r\n\r\nb").length, 2);
  });
  it("cancelletto senza spazio non è un titolo", () => {
    assert.equal(parseMarkdown("#hashtag")[0].t, "paragraph");
  });
});
