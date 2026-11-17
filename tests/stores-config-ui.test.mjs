import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_BOT_SETTINGS,
  INITIAL_CONFIG,
  botImageResolver,
  createConfigStore,
  defaultIntegration,
  enabledEnvs,
  enabledIntegrations,
  patchConfig,
  shownBot,
} from "../src/stores/config.ts";
import {
  INITIAL_UI,
  closeManager,
  closeSettings,
  consumeIntent,
  createUiStore,
  modalOpen,
  openManager,
  openSettings,
  setAddWorkspaceOpen,
  setLocationWorkspace,
  setNotice,
  setOffice,
  setOutdatedPlugins,
  setSessionReady,
  setUpdateAvailable,
  setWorld,
  toggleOffice,
  toggleWorld,
} from "../src/stores/ui.ts";

const IMAGE = { png: "AAAA", color: "#aabbcc" };

describe("config: patchConfig", () => {
  it("stato iniziale", () => {
    assert.equal(INITIAL_CONFIG.botSettings, DEFAULT_BOT_SETTINGS);
    assert.deepEqual(DEFAULT_BOT_SETTINGS, { enabled: true, names: [], images: {}, agents: [] });
    assert.equal(INITIAL_CONFIG.profilesReady, false);
    assert.equal(INITIAL_CONFIG.showUsage, true);
  });

  it("stessi valori (Object.is) = stesso stato", () => {
    assert.equal(patchConfig(INITIAL_CONFIG, {}), INITIAL_CONFIG);
    assert.equal(patchConfig(INITIAL_CONFIG, { showUsage: true, globalLaunch: null }), INITIAL_CONFIG);
    assert.equal(patchConfig(INITIAL_CONFIG, { integrations: INITIAL_CONFIG.integrations }), INITIAL_CONFIG);
  });

  it("un array nuovo, anche se vuoto, è un cambiamento", () => {
    assert.notEqual(patchConfig(INITIAL_CONFIG, { integrations: [] }), INITIAL_CONFIG);
  });

  it("cambia solo i campi indicati e ignora gli undefined", () => {
    const next = patchConfig(INITIAL_CONFIG, { showUsage: false, appVersion: undefined });
    assert.equal(next.showUsage, false);
    assert.equal(next.appVersion, null);
    assert.equal(next.botSettings, INITIAL_CONFIG.botSettings);
  });

  it("lo store non notifica una patch a vuoto", () => {
    const store = createConfigStore();
    let calls = 0;
    store.subscribe(() => calls++);
    store.patch({ showUsage: true });
    assert.equal(calls, 0);
    store.patch({ globalLaunch: "builtin:claude" });
    assert.equal(calls, 1);
    assert.equal(store.getState().globalLaunch, "builtin:claude");
  });
});

describe("config: bot", () => {
  const bots = (extra = {}) => ({ enabled: true, names: ["Bolt", "Nova"], images: { Bolt: IMAGE }, ...extra });

  it("botImageResolver risolve per nome (anche col suffisso numerico)", () => {
    const resolve = botImageResolver(bots());
    assert.equal(resolve("Bolt"), IMAGE);
    assert.equal(resolve("bolt 2"), IMAGE);
    assert.equal(resolve("Nova"), undefined);
    assert.equal(resolve("Sconosciuto"), undefined);
  });

  it("stessa funzione finché names e images sono gli stessi riferimenti", () => {
    const b = bots();
    assert.equal(botImageResolver(b), botImageResolver({ ...b, enabled: false }));
    assert.notEqual(botImageResolver(b), botImageResolver({ ...b, images: { ...b.images } }));
    assert.notEqual(botImageResolver(b), botImageResolver({ ...b, names: [...b.names] }));
  });

  it("l'immagine è sempre lo stesso oggetto (figli memoizzati)", () => {
    const resolve = botImageResolver(bots());
    assert.equal(resolve("Bolt"), resolve("Bolt"));
  });

  it("shownBot mostra il nome solo con la funzione attiva", () => {
    assert.equal(shownBot(bots(), "Bolt"), "Bolt");
    assert.equal(shownBot(bots({ enabled: false }), "Bolt"), undefined);
  });
});

describe("ui: dialoghi", () => {
  it("modalOpen vale per Impostazioni, dialogo di aggiunta e cambio di cartella", () => {
    assert.ok(!modalOpen(INITIAL_UI));
    assert.ok(modalOpen(setAddWorkspaceOpen(INITIAL_UI, true)));
    assert.ok(modalOpen(setLocationWorkspace(INITIAL_UI, "w1")));
    assert.ok(!modalOpen(setLocationWorkspace(setLocationWorkspace(INITIAL_UI, "w1"), null)));
    assert.ok(modalOpen(openSettings(INITIAL_UI, "bots")));
    assert.ok(!modalOpen(closeSettings(openSettings(INITIAL_UI, "bots"))));
  });

  it("openSettings imposta pagina e workspace; aprire di nuovo uguale = stesso stato", () => {
    const s = openSettings(INITIAL_UI, "launches", "w1");
    assert.deepEqual(s.settings, { open: true, tab: "launches", workspaceId: "w1" });
    assert.equal(openSettings(s, "launches", "w1"), s);
    assert.equal(openSettings(s, "launches", undefined).settings.workspaceId, undefined);
  });

  it("closeSettings: chiuso = stesso stato; aperto = solo open falso", () => {
    assert.equal(closeSettings(INITIAL_UI), INITIAL_UI);
    const s = closeSettings(openSettings(INITIAL_UI, "updates"));
    assert.deepEqual(s.settings, { open: false, tab: "updates", workspaceId: undefined });
  });

  it("setAddWorkspaceOpen, setNotice, setUpdateAvailable, setSessionReady: stesso valore = stesso stato", () => {
    assert.equal(setAddWorkspaceOpen(INITIAL_UI, false), INITIAL_UI);
    assert.equal(setLocationWorkspace(INITIAL_UI, null), INITIAL_UI);
    const open = setLocationWorkspace(INITIAL_UI, "w1");
    assert.equal(open.locationWorkspaceId, "w1");
    assert.equal(setLocationWorkspace(open, "w1"), open);
    assert.equal(setNotice(INITIAL_UI, null), INITIAL_UI);
    assert.equal(setUpdateAvailable(INITIAL_UI, false), INITIAL_UI);
    assert.equal(setSessionReady(INITIAL_UI, false), INITIAL_UI);
    assert.equal(setNotice(INITIAL_UI, "ciao").notice, "ciao");
    assert.equal(setSessionReady(INITIAL_UI, true).sessionReady, true);
  });

  it("setOutdatedPlugins: stessa lista = stesso stato", () => {
    const list = [];
    const s = setOutdatedPlugins(INITIAL_UI, list);
    assert.equal(s.outdatedPlugins, list);
    assert.equal(setOutdatedPlugins(s, list), s);
  });
});

describe("ui: Tasks Manager e Ufficio", () => {
  it("openManager apre il manager del workspace e chiude l'Ufficio", () => {
    const s = openManager(setOffice(INITIAL_UI, true), "w1");
    assert.equal(s.managerOpen.w1, true);
    assert.equal(s.officeOpen, false);
    assert.equal(s.managerIntent, null);
  });

  it("openManager con richiesta assegna un nonce crescente", () => {
    const first = openManager(INITIAL_UI, "w1", { taskId: "t1" });
    assert.deepEqual(first.managerIntent, { taskId: "t1", nonce: 1 });
    const second = openManager(first, "w1", { newTask: true });
    assert.deepEqual(second.managerIntent, { newTask: true, nonce: 2 });
    assert.equal(second.intentNonce, 2);
  });

  it("openManager senza richiesta azzera quella pendente ma non consuma nonce", () => {
    const withIntent = openManager(INITIAL_UI, "w1", { taskId: "t1" });
    const s = openManager(withIntent, "w1");
    assert.equal(s.managerIntent, null);
    assert.equal(s.intentNonce, 1);
  });

  it("openManager su un manager già aperto, senza richiesta né Ufficio = stesso stato", () => {
    const s = openManager(INITIAL_UI, "w1");
    assert.equal(openManager(s, "w1"), s);
    assert.notEqual(openManager(s, "w2"), s);
  });

  it("closeManager mette false solo se era aperto", () => {
    const s = openManager(INITIAL_UI, "w1");
    assert.equal(closeManager(s, "w1").managerOpen.w1, false);
    assert.equal(closeManager(INITIAL_UI, "w1"), INITIAL_UI);
    const closed = closeManager(s, "w1");
    assert.equal(closeManager(closed, "w1"), closed);
  });

  it("consumeIntent azzera la richiesta una volta sola", () => {
    const s = openManager(INITIAL_UI, "w1", { taskId: "t1" });
    const consumed = consumeIntent(s);
    assert.equal(consumed.managerIntent, null);
    assert.equal(consumeIntent(consumed), consumed);
  });

  it("setOffice e toggleOffice", () => {
    assert.equal(setOffice(INITIAL_UI, false), INITIAL_UI);
    const open = toggleOffice(INITIAL_UI);
    assert.equal(open.officeOpen, true);
    assert.equal(toggleOffice(open).officeOpen, false);
  });

  it("setWorld e toggleWorld: i terminali di default, a vuoto non cambiano nulla", () => {
    assert.equal(INITIAL_UI.world, "terminals");
    assert.equal(setWorld(INITIAL_UI, "terminals"), INITIAL_UI);
    const hub = toggleWorld(INITIAL_UI);
    assert.equal(hub.world, "hub");
    assert.equal(setWorld(hub, "hub"), hub);
    assert.equal(toggleWorld(hub).world, "terminals");
  });

  it("Ufficio e Agents HUB si escludono a vicenda", () => {
    const hub = setWorld(INITIAL_UI, "hub");
    const office = setOffice(hub, true);
    assert.equal(office.officeOpen, true);
    assert.equal(office.world, "terminals");
    const back = setWorld(office, "hub");
    assert.equal(back.world, "hub");
    assert.equal(back.officeOpen, false);
    assert.equal(toggleOffice(back).world, "terminals");
  });

  it("chiudere l'Ufficio o tornare ai terminali non tocca l'altro", () => {
    const hub = setWorld(INITIAL_UI, "hub");
    assert.equal(setOffice(hub, false), hub);
    const office = setOffice(INITIAL_UI, true);
    assert.equal(setWorld(office, "terminals"), office);
  });

  it("openManager riporta ai terminali", () => {
    const s = openManager(setWorld(INITIAL_UI, "hub"), "w1");
    assert.equal(s.managerOpen.w1, true);
    assert.equal(s.world, "terminals");
  });
});

describe("createUiStore", () => {
  it("le azioni a vuoto non notificano", () => {
    const store = createUiStore();
    let calls = 0;
    store.subscribe(() => calls++);
    store.closeSettings();
    store.closeManager("w1");
    store.consumeIntent();
    store.setNotice(null);
    store.setOffice(false);
    assert.equal(calls, 0);
    store.openManager("w1", { taskId: "t" });
    store.consumeIntent();
    assert.equal(calls, 2);
  });
});

describe("config: selettori delle integrazioni", () => {
  const integration = (id, over = {}) => ({ id, name: id, enabled: true, is_default: false, envs: [], ...over });
  const state = (integrations) => ({ ...INITIAL_CONFIG, integrations });

  it("stato iniziale: nessuna integrazione, nessuna predefinita", () => {
    assert.deepEqual(INITIAL_CONFIG.integrations, []);
    assert.deepEqual(enabledIntegrations(INITIAL_CONFIG), []);
    assert.equal(defaultIntegration(INITIAL_CONFIG), null);
    assert.deepEqual(enabledEnvs(INITIAL_CONFIG), []);
  });

  it("i selettori considerano solo le abilitate", () => {
    const a = integration("a", { is_default: true });
    const b = integration("b", { enabled: false });
    const s = state([a, b]);
    assert.deepEqual(enabledIntegrations(s), [a]);
    assert.equal(defaultIntegration(s), a);
  });

  it("stesso riferimento finché l'elenco non cambia (niente ridisegni)", () => {
    const s = state([integration("a", { is_default: true })]);
    assert.equal(enabledIntegrations(s), enabledIntegrations({ ...s, showUsage: false }));
    assert.equal(enabledEnvs(s), enabledEnvs({ ...s, showUsage: false }));
  });

  it("un elenco nuovo è un cambiamento dello store", () => {
    const store = createConfigStore();
    let calls = 0;
    store.subscribe(() => calls++);
    store.patch({ integrations: [integration("a", { is_default: true })] });
    assert.equal(calls, 1);
    assert.equal(defaultIntegration(store.getState())?.id, "a");
  });
});
