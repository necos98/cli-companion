import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { matchDistro, slashes, toWslPath, wslBrowseStart } from "../src/lib/wslPath.ts";

describe("slashes", () => {
  it("trasforma i backslash in slash e lascia il resto", () => {
    assert.equal(slashes("home\\me\\progetto"), "home/me/progetto");
    assert.equal(slashes("/home/me/progetto"), "/home/me/progetto");
    assert.equal(slashes(""), "");
  });
});

describe("toWslPath", () => {
  it("percorso Linux con i backslash: solo slash", () => {
    assert.deepEqual(toWslPath("\\home\\me\\progetto"), { path: "/home/me/progetto", distro: null });
  });

  it("un percorso già Linux resta com'è", () => {
    for (const p of ["/home/me/progetto", "/", "/mnt/c/Users/me", "progetto/sub"]) {
      assert.deepEqual(toWslPath(p), { path: p, distro: null });
    }
  });

  it("condivisione WSL di Esplora risorse: percorso nella distro e nome della distro", () => {
    assert.deepEqual(toWslPath("\\\\wsl.localhost\\Ubuntu-22.04\\home\\me\\progetto"), {
      path: "/home/me/progetto",
      distro: "Ubuntu-22.04",
    });
    assert.deepEqual(toWslPath("\\\\wsl$\\Ubuntu\\home\\me"), { path: "/home/me", distro: "Ubuntu" });
    assert.deepEqual(toWslPath("\\\\WSL.LOCALHOST\\Debian\\srv"), { path: "/srv", distro: "Debian" });
  });

  it("radice della distro", () => {
    assert.deepEqual(toWslPath("\\\\wsl.localhost\\Ubuntu"), { path: "/", distro: "Ubuntu" });
    assert.deepEqual(toWslPath("\\\\wsl.localhost\\Ubuntu\\"), { path: "/", distro: "Ubuntu" });
  });

  it("unità Windows: /mnt/<lettera minuscola>", () => {
    assert.deepEqual(toWslPath("C:\\Users\\me\\progetto"), { path: "/mnt/c/Users/me/progetto", distro: null });
    assert.deepEqual(toWslPath("D:\\"), { path: "/mnt/d", distro: null });
    assert.deepEqual(toWslPath("e:"), { path: "/mnt/e", distro: null });
  });

  it("toglie spazi, virgolette di 'Copia come percorso' e slash finale", () => {
    assert.deepEqual(toWslPath('  "\\\\wsl.localhost\\Ubuntu\\home\\me\\con spazi\\"  '), {
      path: "/home/me/con spazi",
      distro: "Ubuntu",
    });
    assert.deepEqual(toWslPath("/home/me/progetto/"), { path: "/home/me/progetto", distro: null });
  });

  it("prefisso dei percorsi lunghi \\\\?\\", () => {
    assert.deepEqual(toWslPath("\\\\?\\UNC\\wsl.localhost\\Ubuntu\\home\\me"), { path: "/home/me", distro: "Ubuntu" });
    assert.deepEqual(toWslPath("\\\\?\\C:\\progetti"), { path: "/mnt/c/progetti", distro: null });
  });

  it("una condivisione di rete qualsiasi non è una distro", () => {
    assert.deepEqual(toWslPath("\\\\server\\share\\x"), { path: "//server/share/x", distro: null });
  });

  it("è idempotente", () => {
    for (const p of ["\\\\wsl$\\Ubuntu\\home\\me", "C:\\x\\y", "\\home\\me\\"]) {
      const once = toWslPath(p).path;
      assert.equal(toWslPath(once).path, once);
    }
  });
});

describe("matchDistro", () => {
  const distros = ["Ubuntu", "Debian"];

  it("trova la distro senza distinguere maiuscole e minuscole", () => {
    assert.equal(matchDistro("ubuntu", distros), "Ubuntu");
    assert.equal(matchDistro("Debian", distros), "Debian");
  });

  it("nessun nome o distro non installata: null", () => {
    assert.equal(matchDistro(null, distros), null);
    assert.equal(matchDistro("Arch", distros), null);
  });
});

describe("wslBrowseStart", () => {
  it("parte dalla cartella già scritta", () => {
    assert.equal(wslBrowseStart("Ubuntu", "/home/me/progetto"), "\\\\wsl$\\Ubuntu\\home\\me\\progetto");
    assert.equal(wslBrowseStart("Ubuntu", "\\\\wsl.localhost\\Ubuntu\\srv\\"), "\\\\wsl$\\Ubuntu\\srv");
  });

  it("senza un percorso assoluto parte da /home", () => {
    for (const p of ["", "   ", "progetto", "/"]) {
      assert.equal(wslBrowseStart("Ubuntu", p), "\\\\wsl$\\Ubuntu\\home");
    }
  });
});
