import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { chooseLocation, EMPTY_LOCATION, locationOf, resetsShell } from "../src/lib/workspaceLocation.ts";

describe("locationOf", () => {
  it("Windows: la cartella nel campo Windows, il resto vuoto", () => {
    assert.deepEqual(locationOf({ kind: "windows", path: "C:\\proj" }), { ...EMPTY_LOCATION, winPath: "C:\\proj" });
  });

  it("WSL: distro e percorso Linux, ambiente WSL", () => {
    assert.deepEqual(locationOf({ kind: "wsl", path: "/home/me/proj", distro: "Ubuntu" }), {
      kind: "wsl",
      winPath: "",
      distro: "Ubuntu",
      wslPath: "/home/me/proj",
    });
  });

  it("WSL senza distro (file vecchio): distro vuota, da scegliere", () => {
    assert.equal(locationOf({ kind: "wsl", path: "/srv" }).distro, "");
  });
});

describe("chooseLocation", () => {
  it("Windows: cartella senza spazi esterni e senza distro", () => {
    const fields = { ...EMPTY_LOCATION, winPath: "  D:\\nuova  ", distro: "Ubuntu" };
    assert.deepEqual(chooseLocation(fields), { ok: true, location: { kind: "windows", path: "D:\\nuova" } });
  });

  it("WSL: percorso e distro", () => {
    const fields = { ...EMPTY_LOCATION, kind: "wsl", winPath: "C:\\x", distro: "Debian", wslPath: "/srv/x" };
    assert.deepEqual(chooseLocation(fields), {
      ok: true,
      location: { kind: "wsl", path: "/srv/x", distro: "Debian" },
    });
  });

  it("i campi mancanti danno l'errore da mostrare", () => {
    assert.deepEqual(chooseLocation({ ...EMPTY_LOCATION, winPath: "  " }), {
      ok: false,
      error: "Seleziona una cartella.",
    });
    assert.deepEqual(chooseLocation({ ...EMPTY_LOCATION, kind: "wsl", distro: "Ubuntu" }), {
      ok: false,
      error: "Inserisci il percorso Linux.",
    });
    assert.deepEqual(chooseLocation({ ...EMPTY_LOCATION, kind: "wsl", wslPath: "/srv" }), {
      ok: false,
      error: "Seleziona una distro WSL.",
    });
  });
});

describe("resetsShell", () => {
  it("solo cambiando ambiente con una shell personalizzata", () => {
    assert.equal(resetsShell({ kind: "windows", shell: "pwsh" }, "wsl"), true);
    assert.equal(resetsShell({ kind: "wsl", shell: "zsh" }, "windows"), true);
    assert.equal(resetsShell({ kind: "windows", shell: "pwsh" }, "windows"), false);
    assert.equal(resetsShell({ kind: "windows" }, "wsl"), false);
  });
});
