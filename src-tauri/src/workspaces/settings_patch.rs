//! Proposta di modifica delle impostazioni di un workspace: tipo della patch,
//! lettura degli argomenti del tool, validazione (le stesse funzioni dei comandi
//! dell'interfaccia), applicazione tutto-o-niente e traduzione degli errori.

use super::{
    check_default_launch, clear_default_if_removed, find_ws_mut, next_launch_id, normalize_permission_rules,
    normalize_rules, normalize_worker_model, parse_worker_permission, persist, repo_config_key, validate_launch,
    Delivery, DeliveryMode, E2eConfig, Launch, MergeMethod, RepoConfig, StoreData, Workspace, ALLOW_RULES_LABEL,
    DENY_RULES_LABEL, MAX_E2E_TIMEOUT_MIN, MAX_LAUNCH_NAME_LEN, MIN_E2E_TIMEOUT_MIN, MAX_REPO_COMMANDS, MAX_REPO_COMMAND_LEN, MAX_REPO_PATH_LEN,
    MAX_RULES_LEN,
};
use crate::worker_permissions::{MAX_RULES, MAX_RULE_CHARS};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use tauri::AppHandle;

#[cfg(test)]
mod tests;

/// Massimo di repository in una proposta di impostazioni.
pub const MAX_PATCH_REPOS: usize = 20;
/// Massimo di avvii nella lista proposta.
pub const MAX_PATCH_LAUNCHES: usize = 30;

/// Campi del workspace che una proposta non puo' toccare.
const FIXED_FIELDS: [&str; 8] = ["id", "name", "path", "kind", "distro", "shell", "archived", "created_at"];

/// Prefissi di contesto degli errori di una voce di `launches` / `repos`, e
/// fine del contesto.
const CONTEXT_PREFIXES: [&str; 2] = ["launches[", "repos["];
const CONTEXT_END: &str = "]: ";

/// Un campo `Option<Option<T>>` distingue "assente" (nessuna modifica) da
/// `null` (azzera il valore).
fn de_double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Avvio nella lista proposta: senza `id` e' nuovo (id generato all'applicazione).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub command: String,
}

impl From<&Launch> for LaunchPatch {
    fn from(l: &Launch) -> Self {
        LaunchPatch { id: Some(l.id.clone()), name: l.name.clone(), command: l.command.clone() }
    }
}

/// Campi di consegna da cambiare; gli assenti restano.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DeliveryMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_method: Option<MergeMethod>,
}

impl DeliveryPatch {
    fn is_empty(&self) -> bool {
        *self == DeliveryPatch::default()
    }

    /// `base` con i campi della patch sovrapposti, poi normalizzato.
    fn merge(&self, base: Delivery) -> Delivery {
        Delivery {
            mode: self.mode.unwrap_or(base.mode),
            parallel: self.parallel.unwrap_or(base.parallel),
            merge_method: self.merge_method.unwrap_or(base.merge_method),
        }
        .normalized()
    }

    /// Gli stessi campi della patch, con i valori di `d`.
    fn project(&self, d: Delivery) -> DeliveryPatch {
        DeliveryPatch {
            mode: self.mode.map(|_| d.mode),
            parallel: self.parallel.map(|_| d.parallel),
            merge_method: self.merge_method.map(|_| d.merge_method),
        }
    }
}

/// Modifica parziale della configurazione di un repository: solo i campi
/// presenti cambiano (`null` su `base_branch` / `container_root` li azzera).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoPatch {
    #[serde(default, deserialize_with = "de_double_option", skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<Option<String>>,
    #[serde(default, deserialize_with = "de_double_option", skip_serializing_if = "Option::is_none")]
    pub container_root: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_ignored: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_force: Option<bool>,
    /// Oggetto intero al posto di quello attuale; `null` lo rimuove.
    #[serde(default, deserialize_with = "de_double_option", skip_serializing_if = "Option::is_none")]
    pub e2e: Option<Option<E2eConfig>>,
}

impl RepoPatch {
    pub fn is_empty(&self) -> bool {
        *self == RepoPatch::default()
    }

    /// `base` con i campi della patch sovrapposti, poi normalizzato.
    fn merge(&self, base: &RepoConfig) -> Result<RepoConfig, String> {
        let mut c = base.clone();
        if let Some(v) = &self.base_branch {
            c.base_branch = v.clone();
        }
        if let Some(v) = &self.container_root {
            c.container_root = v.clone();
        }
        if let Some(v) = &self.copy_ignored {
            c.copy_ignored = v.clone();
        }
        if let Some(v) = &self.setup {
            c.setup = v.clone();
        }
        if let Some(v) = &self.cleanup {
            c.cleanup = v.clone();
        }
        if let Some(v) = self.remove_force {
            c.remove_force = v;
        }
        if let Some(v) = &self.e2e {
            c.e2e = v.clone();
        }
        c.normalized()
    }

    /// Gli stessi campi della patch, con i valori di `cfg`.
    fn project(&self, cfg: &RepoConfig) -> RepoPatch {
        RepoPatch {
            base_branch: self.base_branch.as_ref().map(|_| cfg.base_branch.clone()),
            container_root: self.container_root.as_ref().map(|_| cfg.container_root.clone()),
            copy_ignored: self.copy_ignored.as_ref().map(|_| cfg.copy_ignored.clone()),
            setup: self.setup.as_ref().map(|_| cfg.setup.clone()),
            cleanup: self.cleanup.as_ref().map(|_| cfg.cleanup.clone()),
            remove_force: self.remove_force.map(|_| cfg.remove_force),
            e2e: self.e2e.as_ref().map(|_| cfg.e2e.clone()),
        }
    }

    /// Tutta la configurazione, ogni campo presente.
    fn full(cfg: &RepoConfig) -> RepoPatch {
        RepoPatch {
            base_branch: Some(cfg.base_branch.clone()),
            container_root: Some(cfg.container_root.clone()),
            copy_ignored: Some(cfg.copy_ignored.clone()),
            setup: Some(cfg.setup.clone()),
            cleanup: Some(cfg.cleanup.clone()),
            remove_force: Some(cfg.remove_force),
            e2e: Some(cfg.e2e.clone()),
        }
    }
}

/// Modifica proposta alle impostazioni del workspace. Solo i campi presenti
/// cambiano; le impostazioni globali e i dati fissi del workspace non esistono
/// nel tipo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsPatch {
    /// Testo intero; "" = nessuna regola.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_rules: Option<String>,
    /// `null` = modello dell'agente.
    #[serde(deserialize_with = "de_double_option", skip_serializing_if = "Option::is_none")]
    pub worker_model: Option<Option<String>>,
    /// plan | default | accept-edits | bypass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_permission: Option<String>,
    /// Sostituisce l'elenco.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_allow: Option<Vec<String>>,
    /// Sostituisce l'elenco.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_deny: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<DeliveryPatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_schedule: Option<bool>,
    /// `null` = eredita il predefinito globale.
    #[serde(deserialize_with = "de_double_option", skip_serializing_if = "Option::is_none")]
    pub default_launch: Option<Option<String>>,
    /// Lista intera, al posto di quella attuale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launches: Option<Vec<LaunchPatch>>,
    /// Chiave = cartella del repository; valore `null` = rimuove la configurazione.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repos: Option<BTreeMap<String, Option<RepoPatch>>>,
}

impl SettingsPatch {
    /// Legge gli argomenti del tool (oggetto SENZA `reason`). Errori in inglese.
    pub fn parse(v: &serde_json::Value) -> Result<SettingsPatch, String> {
        let obj = v.as_object().ok_or_else(|| "invalid settings: expected a JSON object".to_string())?;
        if let Some(field) = FIXED_FIELDS.iter().find(|f| obj.contains_key(**f)) {
            return Err(format!(
                "`{field}` cannot be changed by a proposal (fixed: {})",
                FIXED_FIELDS.join(", ")
            ));
        }
        SettingsPatch::deserialize(v).map_err(|e| format!("invalid settings: {e}"))
    }

    pub fn is_empty(&self) -> bool {
        *self == SettingsPatch::default()
    }

    /// Valida come i comandi dell'interfaccia e ritorna la patch con chiavi e
    /// valori normalizzati. Errori in italiano (come l'interfaccia).
    pub fn normalized(&self, ws: &Workspace) -> Result<SettingsPatch, String> {
        let launches = self.launches.as_deref().map(|l| normalize_launches(l, &ws.launches)).transpose()?;
        Ok(SettingsPatch {
            worker_rules: self.worker_rules.as_deref().map(normalize_rules).transpose()?,
            worker_model: self.worker_model.clone().map(normalize_worker_model).transpose()?,
            worker_permission: self
                .worker_permission
                .as_deref()
                .map(|m| parse_worker_permission(m).map(|m| m.as_str().to_string()))
                .transpose()?,
            worker_allow: self
                .worker_allow
                .as_deref()
                .map(|r| normalize_permission_rules(r, ALLOW_RULES_LABEL))
                .transpose()?,
            worker_deny: self
                .worker_deny
                .as_deref()
                .map(|r| normalize_permission_rules(r, DENY_RULES_LABEL))
                .transpose()?,
            delivery: self.delivery.as_ref().map(|d| normalize_delivery(d, ws)).transpose()?,
            use_schedule: self.use_schedule,
            default_launch: self.normalized_default_launch(ws, launches.as_deref())?,
            launches,
            repos: self.repos.as_ref().map(|m| normalize_repos(m, ws)).transpose()?,
        })
    }

    /// `default_launch` verificato sulla lista di avvii DOPO la patch.
    fn normalized_default_launch(
        &self,
        ws: &Workspace,
        launches: Option<&[LaunchPatch]>,
    ) -> Result<Option<Option<String>>, String> {
        let Some(choice) = &self.default_launch else { return Ok(None) };
        let Some(id) = choice else { return Ok(Some(None)) };
        let id = id.trim();
        // Gli avvii nuovi non hanno ancora un id: non si possono referenziare.
        let after: Vec<Launch> = match launches {
            Some(list) => ws
                .launches
                .iter()
                .filter(|l| list.iter().any(|p| p.id.as_deref() == Some(l.id.as_str())))
                .cloned()
                .collect(),
            None => ws.launches.clone(),
        };
        check_default_launch(&after, id, &crate::agents::builtin_launch_list())?;
        Ok(Some(Some(id.to_string())))
    }

    /// Applica la patch tutto-o-niente: ogni valore si valida su una copia.
    /// `new_launch_id` genera l'id degli avvii senza `id`.
    pub fn apply(&self, ws: &mut Workspace, new_launch_id: &mut dyn FnMut() -> String) -> Result<(), String> {
        let norm = self.normalized(ws)?;
        let mut next = ws.clone();
        if let Some(r) = norm.worker_rules {
            next.worker_rules = r;
        }
        if let Some(m) = norm.worker_model {
            next.worker_model = m;
        }
        if let Some(p) = norm.worker_permission {
            next.worker_permission = p;
        }
        if let Some(a) = norm.worker_allow {
            next.worker_allow = a;
        }
        if let Some(d) = norm.worker_deny {
            next.worker_deny = d;
        }
        if let Some(d) = norm.delivery {
            next.delivery = d.merge(next.delivery);
        }
        if let Some(u) = norm.use_schedule {
            next.use_schedule = u;
        }
        if let Some(list) = norm.launches {
            replace_launches(&mut next, list, new_launch_id);
        }
        if let Some(d) = norm.default_launch {
            next.default_launch = d;
        }
        for (key, patch) in norm.repos.unwrap_or_default() {
            match patch {
                Some(p) => {
                    let base = next.repos.get(&key).cloned().unwrap_or_default();
                    next.repos.insert(key, p.merge(&base)?);
                }
                None => {
                    next.repos.remove(&key);
                }
            }
        }
        *ws = next;
        Ok(())
    }

    /// Valori attuali dei soli campi toccati (per il confronto nell'interfaccia).
    pub fn current(&self, ws: &Workspace) -> SettingsPatch {
        SettingsPatch {
            worker_rules: self.worker_rules.as_ref().map(|_| ws.worker_rules.clone()),
            worker_model: self.worker_model.as_ref().map(|_| ws.worker_model.clone()),
            worker_permission: self.worker_permission.as_ref().map(|_| ws.worker_permission.clone()),
            worker_allow: self.worker_allow.as_ref().map(|_| ws.worker_allow.clone()),
            worker_deny: self.worker_deny.as_ref().map(|_| ws.worker_deny.clone()),
            delivery: self.delivery.as_ref().map(|d| d.project(ws.delivery)),
            use_schedule: self.use_schedule.map(|_| ws.use_schedule),
            default_launch: self.default_launch.as_ref().map(|_| ws.default_launch.clone()),
            launches: self.launches.as_ref().map(|_| ws.launches.iter().map(LaunchPatch::from).collect()),
            repos: self.repos.as_ref().map(|m| {
                m.iter()
                    .map(|(k, p)| {
                        let current = ws.repos.get(k).map(|cfg| match p {
                            Some(p) => p.project(cfg),
                            None => RepoPatch::full(cfg),
                        });
                        (k.clone(), current)
                    })
                    .collect()
            }),
        }
    }

    /// Campi toccati che eseguono comandi o allargano i permessi, per
    /// l'interfaccia. Ordine: `worker_permission`, `worker_allow`,
    /// `worker_deny`, `launches`, `default_launch`; poi, per ogni repository
    /// (chiavi ordinate) con patch non nulla, `repos[<chiave>].setup` e
    /// `repos[<chiave>].cleanup` se presenti, e `repos[<chiave>].e2e` se
    /// impostato (es. `repos[.].setup`, `repos[api/web].cleanup`,
    /// `repos[.].e2e`). Le rimozioni non sono sensibili.
    pub fn sensitive(&self) -> Vec<String> {
        let direct = [
            ("worker_permission", self.worker_permission.is_some()),
            ("worker_allow", self.worker_allow.is_some()),
            ("worker_deny", self.worker_deny.is_some()),
            ("launches", self.launches.is_some()),
            ("default_launch", self.default_launch.is_some()),
        ];
        let mut out: Vec<String> = direct.iter().filter(|(_, touched)| *touched).map(|(n, _)| n.to_string()).collect();
        for (key, patch) in self.repos.iter().flatten() {
            let Some(p) = patch else { continue };
            if p.setup.is_some() {
                out.push(format!("repos[{key}].setup"));
            }
            if p.cleanup.is_some() {
                out.push(format!("repos[{key}].cleanup"));
            }
            if matches!(p.e2e, Some(Some(_))) {
                out.push(format!("repos[{key}].e2e"));
            }
        }
        out
    }

    /// Consegna o fasce orarie toccate: lo scheduler va riallineato.
    pub fn needs_reconcile(&self) -> bool {
        self.delivery.is_some() || self.use_schedule.is_some()
    }

    /// Chiavi dei repository toccati (normalizzate se la patch lo e').
    pub fn repo_keys(&self) -> Vec<String> {
        self.repos.iter().flatten().map(|(k, _)| k.clone()).collect()
    }
}

fn normalize_delivery(d: &DeliveryPatch, ws: &Workspace) -> Result<DeliveryPatch, String> {
    if d.is_empty() {
        return Err("delivery: has no fields".to_string());
    }
    Ok(d.project(d.merge(ws.delivery)))
}

/// Lista di avvii proposta: ogni voce come l'interfaccia; un `id` deve
/// esistere gia' e comparire una volta sola.
fn normalize_launches(list: &[LaunchPatch], current: &[Launch]) -> Result<Vec<LaunchPatch>, String> {
    if list.len() > MAX_PATCH_LAUNCHES {
        return Err(format!("too many launches in the proposal (max {MAX_PATCH_LAUNCHES})"));
    }
    let mut seen = HashSet::new();
    list.iter()
        .enumerate()
        .map(|(i, p)| normalize_launch(p, current, &mut seen).map_err(|e| format!("launches[{i}{CONTEXT_END}{e}")))
        .collect()
}

fn normalize_launch<'a>(
    p: &'a LaunchPatch,
    current: &[Launch],
    seen: &mut HashSet<&'a str>,
) -> Result<LaunchPatch, String> {
    let (name, command) = validate_launch(&p.name, &p.command)?;
    if let Some(id) = p.id.as_deref() {
        if !current.iter().any(|l| l.id == id) {
            return Err(format!("avvio non trovato: {id}"));
        }
        if !seen.insert(id) {
            return Err(format!("duplicate launch id: {id}"));
        }
    }
    Ok(LaunchPatch { id: p.id.clone(), name, command })
}

/// Sostituisce gli avvii nell'ordine della patch; quelli non elencati sono
/// rimossi (come `remove_launch`: se era il predefinito si torna a ereditare).
fn replace_launches(ws: &mut Workspace, list: Vec<LaunchPatch>, new_launch_id: &mut dyn FnMut() -> String) {
    let old = std::mem::take(&mut ws.launches);
    ws.launches = list
        .into_iter()
        .map(|p| Launch {
            id: p.id.unwrap_or_else(&mut *new_launch_id),
            name: p.name,
            command: p.command,
            is_default: false,
        })
        .collect();
    let removed: Vec<String> =
        old.into_iter().filter(|o| !ws.launches.iter().any(|l| l.id == o.id)).map(|o| o.id).collect();
    for id in &removed {
        clear_default_if_removed(ws, id);
    }
}

/// Repository proposti: chiavi normalizzate, senza doppioni, voci non vuote;
/// ogni errore porta la chiave. `None` (rimozione) passa.
fn normalize_repos(
    map: &BTreeMap<String, Option<RepoPatch>>,
    ws: &Workspace,
) -> Result<BTreeMap<String, Option<RepoPatch>>, String> {
    if map.len() > MAX_PATCH_REPOS {
        return Err(format!("too many repos in the proposal (max {MAX_PATCH_REPOS})"));
    }
    let mut out = BTreeMap::new();
    for (raw, patch) in map {
        let key = repo_config_key(raw).map_err(|e| format!("repos['{raw}']: {e}"))?;
        let in_repo = |e: String| format!("repos['{key}']: {e}");
        if out.contains_key(&key) {
            return Err(in_repo("duplicate repo key".to_string()));
        }
        let entry = match patch {
            None => None,
            Some(p) if p.is_empty() => return Err(in_repo("has no fields".to_string())),
            Some(p) => {
                let base = ws.repos.get(&key).cloned().unwrap_or_default();
                Some(p.project(&p.merge(&base).map_err(in_repo)?))
            }
        };
        out.insert(key, entry);
    }
    Ok(out)
}

/// Messaggi di validazione (prefisso italiano, inglese per il modello). Dove
/// l'italiano continua con `: <dettaglio>` il dettaglio viene mantenuto.
/// Tra prefissi sovrapposti vanno prima i piu' lunghi.
fn message_table() -> Vec<(&'static str, String)> {
    vec![
        ("regole troppo lunghe", format!("worker_rules too long (max {MAX_RULES_LEN} characters)")),
        ("le regole non possono", "worker_rules cannot contain control characters".to_string()),
        ("permesso non valido", "invalid worker_permission (use plan, default, accept-edits or bypass)".to_string()),
        (
            "modello non valido",
            "invalid worker_model (letters, digits, '.', '_' and '-' only, max 64 characters)".to_string(),
        ),
        ("troppe regole", format!("too many rules (max {MAX_RULES})")),
        ("branch base non valido", "invalid base_branch".to_string()),
        ("percorso nel container non valido", "invalid container_root".to_string()),
        (
            "il percorso da copiare deve essere relativo",
            "copy_ignored paths must be relative to the repository".to_string(),
        ),
        (
            "percorso da copiare non valido (niente",
            "invalid copy_ignored path (no '..', not the repository root)".to_string(),
        ),
        (
            "percorso da copiare non valido",
            format!("invalid copy_ignored path (no control characters, max {MAX_REPO_PATH_LEN} characters)"),
        ),
        ("troppi percorsi da copiare", format!("too many copy_ignored paths (max {MAX_REPO_COMMANDS})")),
        ("il comando di setup", "setup commands must be single-line".to_string()),
        ("il comando di pulizia", "cleanup commands must be single-line".to_string()),
        ("comando di setup troppo lungo", format!("setup command too long (max {MAX_REPO_COMMAND_LEN} characters)")),
        (
            "comando di pulizia troppo lungo",
            format!("cleanup command too long (max {MAX_REPO_COMMAND_LEN} characters)"),
        ),
        ("troppi comandi di setup", format!("too many setup commands (max {MAX_REPO_COMMANDS})")),
        ("troppi comandi di pulizia", format!("too many cleanup commands (max {MAX_REPO_COMMANDS})")),
        ("comando E2E mancante", "e2e command is missing".to_string()),
        ("il comando E2E", "e2e command must be single-line".to_string()),
        ("comando E2E troppo lungo", format!("e2e command too long (max {MAX_REPO_COMMAND_LEN} characters)")),
        (
            "timeout E2E non valido",
            format!("invalid e2e timeout_min (from {MIN_E2E_TIMEOUT_MIN} to {MAX_E2E_TIMEOUT_MIN} minutes)"),
        ),
        ("percorso JUnit E2E mancante", "e2e junit path is missing".to_string()),
        ("il percorso JUnit E2E", "e2e junit path must be relative to the repository".to_string()),
        ("percorso JUnit E2E non valido (niente", "invalid e2e junit path (no '..')".to_string()),
        (
            "percorso JUnit E2E non valido",
            format!("invalid e2e junit path (no control characters, max {MAX_REPO_PATH_LEN} characters)"),
        ),
        ("nome avvio mancante","launch name is missing".to_string()),
        ("nome avvio troppo lungo", format!("launch name too long (max {MAX_LAUNCH_NAME_LEN} characters)")),
        ("comando avvio mancante", "launch command is missing".to_string()),
        ("avvio non trovato", "launch not found: an id must be one of the workspace's current launches".to_string()),
        (
            "avvio predefinito non valido",
            "invalid default_launch (use \"shell\", the id of a workspace launch or a builtin launch id)".to_string(),
        ),
    ]
}

/// Motivi di una regola di permesso non valida (suffisso italiano, inglese).
fn rule_reason_table() -> [(String, String); 4] {
    [
        (format!("piu' lunga di {MAX_RULE_CHARS} caratteri"), format!("longer than {MAX_RULE_CHARS} characters")),
        ("contiene a capo o caratteri di controllo".to_string(), "contains newlines or control characters".to_string()),
        (
            "forma attesa: Nome oppure Nome(...), es. Bash(git:*)".to_string(),
            "expected Name or Name(...), e.g. Bash(git:*)".to_string(),
        ),
        ("le regole mcp__ non accettano parentesi".to_string(), "mcp__ rules take no parentheses".to_string()),
    ]
}

/// Liste di regole: etichetta italiana del messaggio e campo della patch.
const RULE_LISTS: [(&str, &str); 2] = [(ALLOW_RULES_LABEL, "worker_allow"), (DENY_RULES_LABEL, "worker_deny")];

/// `regola "<r>": <motivo>` in inglese; "troppe regole" e il resto come gli altri messaggi.
fn translate_rule_error(e: &str) -> String {
    let Some(rest) = e.strip_prefix("regola ") else { return translate_message(e) };
    rule_reason_table()
        .iter()
        .find_map(|(it, en)| rest.strip_suffix(it.as_str()).map(|head| format!("rule {head}{en}")))
        .unwrap_or_else(|| e.to_string())
}

fn translate_message(msg: &str) -> String {
    for (label, field) in RULE_LISTS {
        if let Some(rest) = msg.strip_prefix(label).and_then(|r| r.strip_prefix(": ")) {
            return format!("{field}: {}", translate_rule_error(rest));
        }
    }
    message_table()
        .into_iter()
        .find(|(it, _)| msg.starts_with(it))
        .map(|(it, en)| {
            let detail = &msg[it.len()..];
            if detail.starts_with(':') {
                format!("{en}{detail}")
            } else {
                en
            }
        })
        .unwrap_or_else(|| msg.to_string())
}

/// Separa il contesto iniziale (`repos['<chiave>']: ` o `launches[<i>]: `).
fn split_context(e: &str) -> (&str, &str) {
    if CONTEXT_PREFIXES.iter().any(|p| e.starts_with(p)) {
        if let Some(end) = e.find(CONTEXT_END) {
            return e.split_at(end + CONTEXT_END.len());
        }
    }
    ("", e)
}

/// Messaggio di validazione (italiano, dell'interfaccia) in inglese per il
/// modello; i messaggi sconosciuti o gia' inglesi passano invariati.
pub fn validation_error_en(e: &str) -> String {
    let (context, msg) = split_context(e);
    format!("{context}{}", translate_message(msg))
}

/// Applica una patch alle impostazioni di un workspace e salva su disco
/// (stesso percorso delle impostazioni dell'interfaccia). Ritorna il workspace.
/// Non riallinea lo scheduler: vedi `needs_reconcile` e `reconcile_automation`.
pub fn apply_settings_patch(app: &AppHandle, workspace_id: &str, patch: &SettingsPatch) -> Result<Workspace, String> {
    persist(app, |data| {
        let StoreData { workspaces, counter } = data;
        let ws = find_ws_mut(workspaces, workspace_id)?;
        patch.apply(ws, &mut || next_launch_id(counter))?;
        Ok(ws.clone())
    })
}
