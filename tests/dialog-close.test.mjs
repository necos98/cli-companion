import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const read = (file) => readFileSync(path.join(root, file), "utf8");

// Regressione: una modale si chiude solo con la X o con i pulsanti espliciti.
// Esc e click fuori sono bloccati una volta sola nel wrapper `DialogContent`,
// dopo `{...props}` cosi' una modale non puo' riattivarli per sbaglio.
describe("modali: chiusura solo esplicita", () => {
  const dialog = read("src/components/ui/dialog.tsx");

  it("il wrapper blocca Esc, pointer down e interazione fuori", () => {
    for (const handler of ["onEscapeKeyDown", "onPointerDownOutside", "onInteractOutside"]) {
      assert.ok(dialog.includes(`${handler}={keepOpen(${handler})}`), `${handler} non bloccato`);
    }
  });

  it("i blocchi vengono dopo {...props}", () => {
    assert.match(dialog, /\{\.\.\.props\}\s+onEscapeKeyDown=/, "i blocchi devono seguire {...props}");
  });

  it("keepOpen invoca il gestore del chiamante e poi previene la chiusura", () => {
    assert.match(dialog, /handler\?\.\(event\);\s*event\.preventDefault\(\);/);
  });

  it("le modali non ridefiniscono la chiusura fuori contenuto", () => {
    const files = [
      "PluginUpdateDialog",
      "SettingsDialog",
      "AddWorkspaceDialog",
      "WorkspaceLocationDialog",
      "tasks/NewTaskDialog",
    ];
    for (const file of files) {
      assert.doesNotMatch(read(`src/components/${file}.tsx`), /onEscapeKeyDown|onPointerDownOutside|onInteractOutside/);
    }
  });
});
