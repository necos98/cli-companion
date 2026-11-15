//! Conformita' del contratto `InteractiveLauncher`: righe sicure, ripristino
//! senza skill e, se l'agente le dichiara, istruzioni di sistema applicate
//! all'avvio e al ripristino.

use super::{shell_safe, CONFORMANCE_USER_SKILL};
use crate::agents::contracts::*;
use crate::agents::presets::PRESETS;

/// Nomi di skill e modello che l'avvio interattivo deve rifiutare.
const UNSAFE_SKILL_NAMES: [&str; 4] = ["a$b", "a\"b", "a b", ""];
const UNSAFE_LAUNCH_MODEL: &str = "a b;x";

/// Avvio interattivo: ogni combinazione di modello, effort e skill che l'agente
/// dichiara produce righe sicure che partono dal comando interattivo; il
/// ripristino non riporta mai la skill e l'effort dichiarato e' nelle righe.
pub(super) fn interactive_launch(i: &dyn AgentIntegration, launcher: &dyn InteractiveLauncher) -> Vec<String> {
    let mut v = Vec::new();
    let caps = i.capabilities();
    let models: Vec<Option<&str>> = std::iter::once(None)
        .chain(caps.models.iter().flat_map(|c| c.suggested().iter().map(|m| Some(m.id))))
        .collect();
    let efforts: Vec<Option<Effort>> =
        std::iter::once(None).chain(caps.effort.iter().flat_map(|c| c.levels().iter().copied().map(Some))).collect();
    let mut skills: Vec<Option<LaunchSkill>> = vec![None];
    if caps.plugin_skills.is_some() {
        skills.extend(PRESETS.iter().map(|p| Some(LaunchSkill::Plugin(p.id))));
    }
    if caps.user_skills.is_some() {
        skills.push(Some(LaunchSkill::User(CONFORMANCE_USER_SKILL)));
    }
    let interactive = i.interactive_command();
    for &model in &models {
        for &effort in &efforts {
            for &skill in &skills {
                let req = InteractiveRequest { model, effort, skill, instructions_file: None };
                let Some(launch) = launcher.launch(&req) else {
                    v.push(format!("interactive_launch: nessun avvio per {req:?}"));
                    continue;
                };
                let InteractiveLaunch { command, restore_command, env, restore_env } = &launch;
                for (what, line) in [("command", command), ("restore_command", restore_command)] {
                    if !shell_safe(line) {
                        v.push(format!("interactive_launch: {what} non sicuro per la shell ({req:?}): {line:?}"));
                    }
                    if !line.starts_with(interactive) {
                        v.push(format!(
                            "interactive_launch: {what} non parte dal comando interattivo ({req:?}): {line:?}"
                        ));
                    }
                    let args = effort.and_then(|e| caps.effort?.command_args(e));
                    if args.is_some_and(|args| !line.contains(&args)) {
                        v.push(format!(
                            "interactive_launch: {what} senza gli argomenti dell'effort ({req:?}): {line:?}"
                        ));
                    }
                }
                if skill.is_none() && command != restore_command {
                    v.push(format!("interactive_launch: senza skill command e restore_command differiscono ({req:?})"));
                }
                if let Some(LaunchSkill::Plugin(name) | LaunchSkill::User(name)) = skill {
                    if restore_command.contains(name) {
                        v.push(format!("interactive_launch: restore_command riporta la skill {name:?} ({req:?})"));
                    }
                }
                for (what, vars) in [("env", env), ("restore_env", restore_env)] {
                    if let Some(name) = first_invalid_env_name(vars) {
                        v.push(format!(
                            "interactive_launch: {what} con variabile d'ambiente non valida o duplicata ({req:?}): {name:?}"
                        ));
                    }
                }
                if let Some(LaunchSkill::Plugin(name) | LaunchSkill::User(name)) = skill {
                    if restore_env.iter().any(|(_, value)| value.contains(name)) {
                        v.push(format!("interactive_launch: restore_env riporta la skill {name:?} ({req:?})"));
                    }
                }
            }
        }
    }
    for name in UNSAFE_SKILL_NAMES {
        for skill in [LaunchSkill::Plugin(name), LaunchSkill::User(name)] {
            let req = InteractiveRequest { model: None, effort: None, skill: Some(skill), instructions_file: None };
            if launcher.launch(&req).is_some() {
                v.push(format!("interactive_launch: skill non sicura accettata: {skill:?}"));
            }
        }
    }
    let req =
        InteractiveRequest { model: Some(UNSAFE_LAUNCH_MODEL), effort: None, skill: None, instructions_file: None };
    if launcher.launch(&req).is_some() {
        v.push(format!("interactive_launch: modello non sicuro accettato: {UNSAFE_LAUNCH_MODEL:?}"));
    }
    if launcher.applies_instructions() {
        v.extend(instructions_file(launcher));
    }
    v
}

/// File di istruzioni della suite: sicuro per la shell, con spazi e slash dritti
/// (cosi' si riconosce anche dentro un valore JSON), e uno da rifiutare.
const CONFORMANCE_INSTRUCTIONS_FILE: &str =
    "C:/Users/Mario Rossi/AppData/Roaming/cli-companion/bot-instructions/standard.md";
const UNSAFE_INSTRUCTIONS_FILE: &str = "C:/a$b/standard.md";

/// Un agente che dichiara di applicare le istruzioni le porta all'avvio e al
/// ripristino (nella riga o nell'ambiente) e rifiuta un file non sicuro.
fn instructions_file(launcher: &dyn InteractiveLauncher) -> Vec<String> {
    let mut v = Vec::new();
    let request = |file| InteractiveRequest { model: None, effort: None, skill: None, instructions_file: Some(file) };
    let carries = |line: &str, env: &[(String, String)]| {
        line.contains(CONFORMANCE_INSTRUCTIONS_FILE)
            || env.iter().any(|(_, value)| value.contains(CONFORMANCE_INSTRUCTIONS_FILE))
    };
    match launcher.launch(&request(CONFORMANCE_INSTRUCTIONS_FILE)) {
        None => v.push(format!(
            "interactive_launch: file di istruzioni sicuro rifiutato: {CONFORMANCE_INSTRUCTIONS_FILE:?}"
        )),
        Some(launch) => {
            if !carries(&launch.command, &launch.env) {
                v.push("interactive_launch: file di istruzioni dichiarato ma non applicato all'avvio".to_string());
            }
            if !carries(&launch.restore_command, &launch.restore_env) {
                v.push("interactive_launch: file di istruzioni dichiarato ma perso al ripristino".to_string());
            }
        }
    }
    if launcher.launch(&request(UNSAFE_INSTRUCTIONS_FILE)).is_some() {
        v.push(format!("interactive_launch: file di istruzioni non sicuro accettato: {UNSAFE_INSTRUCTIONS_FILE:?}"));
    }
    v
}
