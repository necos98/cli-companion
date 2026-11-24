// Test architetturale del frontend: non sa come funziona un coding agent. Un
// agente specifico non si nomina in `src/`: l'interfaccia agisce sui dati
// dichiarativi dei contratti (`agent_profiles`). Esclusi commenti, test e
// fixture. Guida: docs/wiki/integrare-un-coding-agent.md
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const SRC = path.join(root, "src");
const WIKI_PAGE = "docs/wiki/integrare-un-coding-agent.md";

/** Integrazioni note. Aggiungendo un agente: una riga qui (e nel test Rust). */
const INTEGRATIONS = [
  { module: "claude_code", id: "claude-code", commands: ["claude", "claude.exe"], models: ["sonnet", "opus", "haiku"] },
  {
    module: "opencode",
    id: "opencode",
    commands: ["opencode", "opencode.exe", "opencode.cmd"],
    models: ["big-pickle", "gpt-5.5", "gpt-5.4-mini"],
  },
  { module: "pi_agent", id: "pi-agent", commands: ["pi", "pi.cmd"], models: [] },
];

const SOURCE_FILE = /\.(ts|tsx|js|jsx|mjs)$/;
const TEST_OR_FIXTURE = /(\.test\.|\.spec\.|[\\/](__tests__|__fixtures__|fixtures)[\\/])/;
/** Caratteri dopo i quali una `/` apre una regex e non e' una divisione. */
const REGEX_PRECEDERS = new Set("(,=:[!&|?{};+-*%<>~^");

/**
 * Stringhe e identificatori di un sorgente JS/TS, senza commenti.
 * @returns {{kind: "str" | "ident", text: string, line: number}[]}
 */
function tokenize(source) {
  const tokens = [];
  let i = 0;
  let line = 1;
  let previous = "";

  const skipBlockComment = () => {
    const end = source.indexOf("*/", i + 2);
    const stop = end === -1 ? source.length : end + 2;
    line += source.slice(i, stop).split("\n").length - 1;
    i = stop;
  };
  const skipRegex = () => {
    let inClass = false;
    for (i += 1; i < source.length; i += 1) {
      const c = source[i];
      if (c === "\\") i += 1;
      else if (c === "[") inClass = true;
      else if (c === "]") inClass = false;
      else if (c === "/" && !inClass) break;
      else if (c === "\n") break;
    }
    i += 1;
  };
  const readQuoted = (quote) => {
    const start = line;
    let text = "";
    for (i += 1; i < source.length && source[i] !== quote; i += 1) {
      if (source[i] === "\\") {
        text += source[i];
        i += 1;
      }
      if (source[i] === "\n") line += 1;
      text += source[i] ?? "";
    }
    i += 1;
    tokens.push({ kind: "str", text, line: start });
  };
  // Template: il testo e' una stringa, le espressioni `${...}` si leggono come codice.
  const readTemplate = () => {
    let text = "";
    let start = line;
    const flush = () => {
      tokens.push({ kind: "str", text, line: start });
      text = "";
    };
    for (i += 1; i < source.length && source[i] !== "`"; i += 1) {
      if (source[i] === "\\") {
        text += source[i];
        i += 1;
      } else if (source[i] === "$" && source[i + 1] === "{") {
        flush();
        i += 2;
        readCode(true);
        start = line;
        continue;
      }
      if (source[i] === "\n") line += 1;
      text += source[i] ?? "";
    }
    i += 1;
    flush();
  };
  function readCode(untilBrace) {
    let depth = 0;
    while (i < source.length) {
      const c = source[i];
      if (c === "\n") {
        line += 1;
        i += 1;
      } else if (/\s/.test(c)) {
        i += 1;
      } else if (c === "/" && source[i + 1] === "/") {
        while (i < source.length && source[i] !== "\n") i += 1;
      } else if (c === "/" && source[i + 1] === "*") {
        skipBlockComment();
      } else if (c === "/" && (previous === "" || REGEX_PRECEDERS.has(previous))) {
        skipRegex();
        previous = ")";
      } else if (c === '"' || c === "'") {
        readQuoted(c);
        previous = '"';
      } else if (c === "`") {
        readTemplate();
        previous = '"';
      } else if (/[\p{L}\p{N}_$]/u.test(c)) {
        let text = "";
        while (i < source.length && /[\p{L}\p{N}_$]/u.test(source[i])) text += source[i++];
        tokens.push({ kind: "ident", text, line });
        previous = "a";
      } else {
        if (untilBrace && c === "{") depth += 1;
        if (untilBrace && c === "}" && depth-- === 0) {
          i += 1;
          return;
        }
        previous = c;
        i += 1;
      }
    }
  }
  readCode(false);
  return tokens;
}

/** Riferimenti a integrazioni nei token di un file (path relativo a `src/`). */
function violationsIn(file, tokens, integrations = INTEGRATIONS) {
  const found = [];
  for (const { module, id, commands, models } of integrations) {
    for (const token of tokens) {
      if (token.kind === "ident" && token.text === module) {
        found.push({ file, line: token.line, what: `il modulo \`${module}\`` });
      } else if (token.kind === "str" && token.text.includes(id)) {
        found.push({ file, line: token.line, what: `la stringa ${JSON.stringify(token.text)} (id \`${id}\`)` });
      } else if (token.kind === "str" && token.text.includes(module)) {
        found.push({ file, line: token.line, what: `la stringa ${JSON.stringify(token.text)} (modulo \`${module}\`)` });
      } else if (token.kind === "str" && models.some((model) => token.text.toLowerCase().includes(model))) {
        found.push({
          file,
          line: token.line,
          what: `il modello nella stringa ${JSON.stringify(token.text)} (di \`${id}\`)`,
        });
      } else if (token.kind === "str" && commands.includes(token.text)) {
        found.push({ file, line: token.line, what: `il comando ${JSON.stringify(token.text)} (di \`${id}\`)` });
      }
    }
  }
  return found;
}

function sourceFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(full);
    return SOURCE_FILE.test(entry.name) && !TEST_OR_FIXTURE.test(full) ? [full] : [];
  });
}

describe("architettura: il frontend non nomina un agente specifico", () => {
  it("nessun riferimento a un'integrazione in src/", () => {
    const found = sourceFiles(SRC).flatMap((file) =>
      violationsIn(path.relative(SRC, file).replaceAll("\\", "/"), tokenize(readFileSync(file, "utf8"))),
    );
    const lines = found.map((v) => `  src/${v.file}:${v.line}: ${v.what}`);
    assert.deepEqual(
      found,
      [],
      `Il frontend nomina un agente specifico (agisce sui dati dei contratti, non conosce gli agenti).\n${lines.join("\n")}\n` +
        `Leggi il dato da agent_profiles invece di nominare l'agente. Guida: ${WIKI_PAGE}`,
    );
  });

  it("il lettore ignora commenti, regex e test, e trova stringhe e template", () => {
    const source = [
      '// "claude" e claude_code',
      '/* "claude-code" */',
      'const re = /"claude"/g; const half = 1 / 2;',
      'const ok = "claudette";',
      'const a = "claude-code";',
      "const b = `x ${cmd === 'claude.exe'} y`;",
    ].join("\n");
    const found = violationsIn("a.ts", tokenize(source));
    assert.deepEqual(
      found.map((v) => [v.line, v.what]),
      [
        [5, 'la stringa "claude-code" (id `claude-code`)'],
        [6, 'il comando "claude.exe" (di `claude-code`)'],
      ],
    );
  });

  it("trova il modulo come identificatore e dentro un import", () => {
    const found = violationsIn("a.ts", tokenize('import x from "./claude_code/x"; const claude_code = 1;'));
    assert.deepEqual(
      found.map((v) => v.what),
      ['la stringa "./claude_code/x" (modulo `claude_code`)', "il modulo `claude_code`"],
    );
  });

  it("trova i nomi dei modelli nelle stringhe, senza distinguere le maiuscole", () => {
    const source = [
      "// sonnet e opus nei commenti",
      'const a = "Sonnet";',
      "const b = `predefinito (es. ${x} claude-OPUS-4)`;",
      'const c = "predefinito dell’agente";',
    ].join("\n");
    const found = violationsIn("a.ts", tokenize(source));
    assert.deepEqual(
      found.map((v) => [v.line, v.what]),
      [
        [2, 'il modello nella stringa "Sonnet" (di `claude-code`)'],
        [3, 'il modello nella stringa "claude-OPUS-4)" (di `claude-code`)'],
      ],
    );
  });

  it("test e fixture non vengono letti", () => {
    for (const file of ["a/b.test.ts", "a/b.spec.tsx", "src/__tests__/x.ts", "src/fixtures/x.ts"]) {
      assert.ok(TEST_OR_FIXTURE.test(file), file);
    }
    assert.ok(!TEST_OR_FIXTURE.test("src/lib/agents.ts"));
  });
});

/**
 * Connettori noti: il frontend e' generico e si costruisce dal descrittore, quindi non ne
 * nomina nessuno. Aggiungendo un connettore: una riga qui (e in `connectors/architecture.rs`).
 * @type {{ id: string }[]}
 */
const CONNECTORS = [{ id: "gmail" }, { id: "telegram" }];

/** Un connettore nella forma attesa da `violationsIn` (modulo = id con underscore). */
const asIntegration = ({ id }) => ({ module: id.replaceAll("-", "_"), id, commands: [], models: [] });

describe("architettura: il frontend non nomina un connettore specifico", () => {
  it("nessun riferimento a un connettore in src/", () => {
    const connectors = CONNECTORS.map(asIntegration);
    const found = sourceFiles(SRC).flatMap((file) =>
      violationsIn(path.relative(SRC, file).replaceAll("\\", "/"), tokenize(readFileSync(file, "utf8")), connectors),
    );
    const lines = found.map((v) => `  src/${v.file}:${v.line}: ${v.what}`);
    assert.deepEqual(
      found,
      [],
      `Il frontend nomina un connettore specifico (costruisce l'interfaccia dal descrittore).\n${lines.join("\n")}\n` +
        "Leggi il dato dal descrittore invece di nominare il connettore. Guida: docs/CONNECTORS.MD",
    );
  });

  it("il controllo trova un connettore nelle stringhe e come identificatore (caso sintetico)", () => {
    const connectors = [asIntegration({ id: "fake-mail" })];
    const source = [
      '// "fake-mail" nei commenti non conta',
      'const a = "fake-mail";',
      "const fake_mail = 1;",
      'const ok = "fakemail";',
    ].join("\n");
    const found = violationsIn("a.ts", tokenize(source), connectors);
    assert.deepEqual(
      found.map((v) => [v.line, v.what]),
      [
        [2, 'la stringa "fake-mail" (id `fake-mail`)'],
        [3, "il modulo `fake_mail`"],
      ],
    );
  });
});
