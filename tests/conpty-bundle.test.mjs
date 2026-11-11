// Il ConPTY distribuito con l'app (src-tauri/windows/conpty/): il bundle deve
// installare conpty.dll e OpenConsole.exe accanto all'eseguibile, altrimenti
// portable-pty ripiega in silenzio sul ConPTY di sistema, e i file devono
// essere quelli dichiarati nel README (versione e SHA-256).
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const TAURI_DIR = path.join(root, "src-tauri");
const CONPTY_DIR = "windows/conpty";

/** File del pacchetto Microsoft.Windows.Console.ConPTY 1.25.260930003 (x64). */
const FILES = [
  { name: "conpty.dll", sha256: "feeef341d891643c62d30b6b07800bc70f0bc148f44cb8c3bee84aa557ae805a" },
  { name: "OpenConsole.exe", sha256: "3d66b23d0a71bb8eed2b77edc8b9df9bf54ce6c8fb74c863a30e760997f80586" },
];

const config = JSON.parse(readFileSync(path.join(TAURI_DIR, "tauri.conf.json"), "utf8"));
const readme = readFileSync(path.join(TAURI_DIR, CONPTY_DIR, "README.md"), "utf8");

describe("ConPTY nel bundle", () => {
  for (const { name, sha256 } of FILES) {
    it(`${name} finisce accanto all'eseguibile`, () => {
      // La destinazione e' relativa alla cartella delle risorse, che su Windows
      // e' quella dell'eseguibile: deve essere il solo nome del file.
      assert.equal(config.bundle.resources?.[`${CONPTY_DIR}/${name}`], name);
    });

    it(`${name} e' il file dichiarato nel README`, () => {
      const actual = createHash("sha256")
        .update(readFileSync(path.join(TAURI_DIR, CONPTY_DIR, name)))
        .digest("hex");
      assert.equal(actual, sha256);
      assert.ok(readme.includes(sha256), `SHA-256 di ${name} assente dal README`);
    });
  }
});
