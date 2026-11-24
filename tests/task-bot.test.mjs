import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  BOT_SETTINGS_PATH,
  GENERAL_BOT_ID,
  INTERACTIVE_ONLY_LABEL,
  NO_BOT_LABEL,
  TASK_KIND_LABELS,
  UNSELECTABLE_BOT_SUFFIX,
  botAfterKindChange,
  botAgentId,
  botById,
  botHasKind,
  botKindError,
  botName,
  botOptions,
  botsForKind,
  defaultBotChoice,
  kindsSummary,
} from "../src/lib/taskBot.ts";
import { taskAgents } from "../src/lib/taskAgent.ts";
import { botChange, buildPatch, formFromTask, validateTaskForm } from "../src/lib/taskForm.ts";

const bot = (id, over = {}) => ({
  id,
  name: `Bot ${id}`,
  task: "Does things",
  agent: null,
  model: { kind: "inherit" },
  effort: null,
  skill: null,
  kinds: ["work"],
  enabled: true,
  image: null,
  preset: null,
  ...over,
});

const GENERAL = bot(GENERAL_BOT_ID, { name: "Generale", kinds: ["work", "plan"] });
const REVIEWER = bot("reviewer", { name: "Revisore", agent: "beta", kinds: ["review"] });
const OFF = bot("off", { name: "Spento", enabled: false, kinds: ["work", "review"] });
const BOTS = [REVIEWER, OFF, GENERAL];

describe("ricerca del Bot", () => {
  it("botById: il Bot se esiste, altrimenti niente", () => {
    assert.equal(botById(BOTS, "reviewer"), REVIEWER);
    assert.equal(botById(BOTS, "ignoto"), undefined);
    assert.equal(botById(BOTS, undefined), undefined);
    assert.equal(botById(BOTS, ""), undefined);
  });
  it("botName: il nome, l'id se il Bot è stato cancellato, null senza Bot", () => {
    assert.equal(botName(BOTS, "reviewer"), "Revisore");
    assert.equal(botName(BOTS, "cancellato"), "cancellato");
    assert.equal(botName(BOTS, undefined), null);
    assert.equal(botName(BOTS, ""), null);
  });
});

describe("scelta del Bot nei form", () => {
  it("solo i Bot abilitati, nell'ordine dei dati", () => {
    assert.deepEqual(botOptions(BOTS, "general", "plan"), [{ id: GENERAL_BOT_ID, label: "Generale" }]);
    assert.deepEqual(botOptions(BOTS, "reviewer", "review"), [{ id: "reviewer", label: "Revisore" }]);
  });
  it("un Bot disabilitato già assegnato resta in coda, marcato", () => {
    const options = botOptions(BOTS, "off", "work");
    assert.deepEqual(options.at(-1), { id: "off", label: `Spento ${UNSELECTABLE_BOT_SUFFIX}` });
    assert.equal(options.length, 2);
  });
  it("un Bot cancellato resta in coda col suo id", () => {
    assert.deepEqual(botOptions(BOTS, "cancellato", "work").at(-1), {
      id: "cancellato",
      label: `cancellato ${UNSELECTABLE_BOT_SUFFIX}`,
    });
  });
  it("«Nessun Bot» c'è solo per chi non ne ha uno", () => {
    assert.deepEqual(botOptions(BOTS, "", "work")[0], { id: "", label: NO_BOT_LABEL });
    assert.ok(!botOptions(BOTS, "reviewer", "review").some((o) => o.id === ""));
  });
  it("senza Bot: solo «Nessun Bot»", () => {
    assert.deepEqual(botOptions([], "", "work"), [{ id: "", label: NO_BOT_LABEL }]);
  });
  it("preselezione: general, altrimenti il primo abilitato, altrimenti niente", () => {
    const anyBot = bot("any", { kinds: ["work", "review", "plan"] });
    assert.equal(defaultBotChoice(BOTS, "work"), GENERAL_BOT_ID);
    assert.equal(defaultBotChoice([OFF, anyBot], "work"), "any");
    assert.equal(defaultBotChoice([bot(GENERAL_BOT_ID, { enabled: false }), anyBot], "work"), "any");
    assert.equal(defaultBotChoice([OFF], "work"), "");
    assert.equal(defaultBotChoice([], "work"), "");
  });
  it("preselezione per tipo: general se lo esegue, altrimenti il primo che lo esegue", () => {
    assert.equal(defaultBotChoice(BOTS, "plan"), GENERAL_BOT_ID);
    assert.equal(defaultBotChoice(BOTS, "review"), "reviewer", "general non fa review: tocca al Revisore");
    assert.equal(defaultBotChoice([GENERAL, REVIEWER], "review"), "reviewer");
    assert.equal(defaultBotChoice([GENERAL], "review"), "", "nessun Bot per la review");
  });
});

describe("tipi di task dei Bot", () => {
  it("etichette italiane, le stesse dei badge", () => {
    assert.deepEqual(TASK_KIND_LABELS, {
      work: "Lavoro",
      review: "Review",
      plan: "Pianificazione",
    });
  });
  it("botHasKind e botsForKind: solo abilitati e con quel tipo, nell'ordine dei dati", () => {
    assert.equal(botHasKind(REVIEWER, "review"), true);
    assert.equal(botHasKind(REVIEWER, "work"), false);
    assert.deepEqual(botsForKind(BOTS, "review"), [REVIEWER], "il Bot spento non c'è");
    assert.deepEqual(botsForKind(BOTS, "work"), [GENERAL]);
    assert.deepEqual(botsForKind(BOTS, "plan"), [GENERAL]);
  });
  it("un Bot senza tipi (solo interattivo) non è mai selezionabile", () => {
    const interactive = bot("tab", { kinds: [] });
    assert.deepEqual(botsForKind([interactive], "work"), []);
    assert.equal(defaultBotChoice([interactive], "work"), "");
  });
  it("riepilogo: etichette nell'ordine dei tipi, vuoto = solo terminali interattivi", () => {
    assert.equal(kindsSummary(["plan", "work"]), "Lavoro, Pianificazione");
    assert.equal(kindsSummary(["review"]), "Review");
    assert.equal(kindsSummary([]), INTERACTIVE_ONLY_LABEL);
    assert.equal(INTERACTIVE_ONLY_LABEL, "Solo terminali interattivi");
  });
});

describe("cambio di tipo nel form", () => {
  it("il Bot compatibile resta, senza avviso", () => {
    assert.deepEqual(botAfterKindChange(BOTS, GENERAL_BOT_ID, "plan"), { bot: GENERAL_BOT_ID, notice: null });
  });
  it("un task senza Bot (vecchio) resta senza", () => {
    assert.deepEqual(botAfterKindChange(BOTS, "", "review"), { bot: "", notice: null });
  });
  it("Bot non compatibile: si passa al default del tipo e lo si dice", () => {
    const change = botAfterKindChange(BOTS, GENERAL_BOT_ID, "review");
    assert.equal(change.bot, "reviewer");
    assert.match(change.notice, /Generale non esegue i task di tipo Review/);
    assert.match(change.notice, /scelto Revisore/);
  });
  it("Bot disabilitato o cancellato: si passa al default", () => {
    assert.equal(botAfterKindChange(BOTS, "off", "work").bot, GENERAL_BOT_ID);
    assert.equal(botAfterKindChange(BOTS, "cancellato", "work").bot, GENERAL_BOT_ID);
  });
  it("nessun Bot compatibile: il Bot si svuota e l'avviso non promette un'alternativa", () => {
    const change = botAfterKindChange([GENERAL], GENERAL_BOT_ID, "review");
    assert.equal(change.bot, "");
    assert.doesNotMatch(change.notice, /scelto/);
  });
});

describe("errore se nessun Bot esegue il tipo", () => {
  it("rimanda a Impostazioni › Bot", () => {
    const message = botKindError([GENERAL], "review");
    assert.match(message, /Review/);
    assert.ok(message.includes(BOT_SETTINGS_PATH));
  });
  it("nessun errore se c'è un Bot compatibile o non ci sono Bot (task vecchi)", () => {
    assert.equal(botKindError(BOTS, "review"), undefined);
    assert.equal(botKindError([], "review"), undefined);
  });
  it("validateTaskForm lo porta nel campo bot e blocca l'invio", () => {
    const values = { title: "t", task: "s", model: "", depends_on: ["a"], kind: "review" };
    assert.ok(validateTaskForm(values, [GENERAL]).bot);
    assert.deepEqual(validateTaskForm(values, BOTS), {});
    assert.deepEqual(validateTaskForm(values), {}, "senza elenco dei Bot non si blocca");
  });
});

describe("agente del Bot", () => {
  it("il suo, altrimenti quello del workspace", () => {
    assert.equal(botAgentId(BOTS, "reviewer", "alpha"), "beta");
    assert.equal(botAgentId(BOTS, GENERAL_BOT_ID, "alpha"), "alpha");
  });
  it("Bot ignoto o niente agente noto: vuoto", () => {
    assert.equal(botAgentId(BOTS, "ignoto", "alpha"), "");
    assert.equal(botAgentId(BOTS, GENERAL_BOT_ID, undefined), "");
  });
});

describe("i Bot dei task", () => {
  it("taskAgents li porta con sé; senza, la lista è vuota", () => {
    const ws = { kind: "windows", worker_agents: {} };
    assert.deepEqual(taskAgents([], ws, BOTS).bots, BOTS);
    assert.deepEqual(taskAgents([], ws).bots, []);
  });
});

describe("Bot nella patch del form", () => {
  const stored = {
    id: "t",
    title: "T",
    task: "S",
    depends_on: [],
    repo: null,
    agent: "alpha",
    bot: GENERAL_BOT_ID,
    model: "a-small",
    kind: "work",
  };
  const legacy = { ...stored, bot: undefined };

  it("il form parte dal Bot del task, vuoto per i task vecchi", () => {
    assert.equal(formFromTask(stored).bot, GENERAL_BOT_ID);
    assert.equal(formFromTask(legacy).bot, "");
  });
  it("Bot invariato: nessun campo bot", () => {
    assert.deepEqual(buildPatch(stored, formFromTask(stored), false), {});
    assert.deepEqual(buildPatch(legacy, formFromTask(legacy), false), {});
  });
  it("cambiare Bot svuota il modello e il backend riallinea il resto: solo bot e model", () => {
    const form = { ...formFromTask(stored), ...botChange("reviewer") };
    assert.deepEqual(buildPatch(stored, form, false), { bot: "reviewer", model: null });
  });
  it("cambiare Bot e scrivere un modello: il modello è un override nella patch", () => {
    const form = { ...formFromTask(stored), ...botChange("reviewer"), model: "b-only" };
    assert.deepEqual(buildPatch(stored, form, false), { bot: "reviewer", model: "b-only" });
  });
  it("un task vecchio che prende un Bot", () => {
    const form = { ...formFromTask(legacy), ...botChange("reviewer") };
    assert.deepEqual(buildPatch(legacy, form, false), { bot: "reviewer", model: null });
  });
  it("tornare a «Nessun Bot» azzera il Bot", () => {
    const form = { ...formFromTask(stored), bot: "" };
    assert.deepEqual(buildPatch(stored, form, false), { bot: null });
  });
  it("botChange non tocca l'agente", () => {
    assert.deepEqual(botChange("reviewer"), { bot: "reviewer", model: "" });
  });
});
