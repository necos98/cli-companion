//! Tabella unica dei metodi RPC e dei ruoli che possono chiamarli. `dispatch`
//! la usa per il controllo di accesso e lo smistamento, il server MCP per
//! scegliere i tool di ogni ruolo (un test verifica che coincidano): aggiungere
//! un metodo obbliga a decidere chi lo puo' chiamare, perche' `Method::roles`
//! e' un `match` esaustivo.

use crate::board::TaskKind;

/// Chi sta chiamando, come lo riporta `whoami`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Qualunque tab di cli-companion che non e' il worker di un task (Intake compreso).
    Session,
    /// Il worker di un task `work`.
    Worker,
    /// Il worker di un task `review`: verifica il risultato integrato e propone le correzioni.
    Reviewer,
    /// Il worker di un task `plan`: esplora il repository in sola lettura e propone i task.
    Planner,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Session, Role::Worker, Role::Reviewer, Role::Planner];

    /// Valore di `whoami` sul filo.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Session => "session",
            Role::Worker => "worker",
            Role::Reviewer => "reviewer",
            Role::Planner => "planner",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.as_str() == s)
    }

    /// Ruolo di un tab dal tipo del task che sta eseguendo (`None` = nessun task).
    pub fn of_worker(kind: Option<TaskKind>) -> Role {
        match kind {
            None => Role::Session,
            Some(TaskKind::Work) => Role::Worker,
            Some(TaskKind::Review) => Role::Reviewer,
            Some(TaskKind::Plan) => Role::Planner,
        }
    }
}

/// Metodo RPC, con il nome che usa sul filo (e come tool MCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Handshake del server MCP: non e' un tool.
    Whoami,
    GetTask,
    CompleteTask,
    RunE2e,
    GetE2eResult,
    ProposeTasks,
    ListTasks,
    GetTaskDetails,
    UpdateTask,
    DeleteTask,
    SaveSkill,
    ListSkills,
    DeleteSkill,
    GetWorkspaceSettings,
    ProposeSettings,
    /// Domanda bloccante all'utente: solo con l'Agents HUB acceso (vedi `requires_hub`).
    AskUser,
    ListGoals,
    GetGoal,
}

/// Tutti i ruoli.
const EVERYONE: &[Role] = &Role::ALL;
/// I worker automatici: leggono il loro task e ne riferiscono l'esito.
const AUTOMATED: &[Role] = &[Role::Worker, Role::Reviewer, Role::Planner];
/// Chi verifica il codice con gli E2E: il pianificatore non scrive codice.
const E2E_RUNNERS: &[Role] = &[Role::Worker, Role::Reviewer];
/// Proposte e dettaglio dei task: il revisore e il pianificatore ne hanno bisogno.
const PROPOSERS: &[Role] = &[Role::Session, Role::Reviewer, Role::Planner];
/// Lettura della coda e dei goal: anche il pianificatore.
const SESSION_AND_PLANNER: &[Role] = &[Role::Session, Role::Planner];
/// Dettaglio di un goal: anche il revisore, che verifica l'obiettivo.
const GOAL_READERS: &[Role] = &[Role::Session, Role::Reviewer, Role::Planner];
/// Lettura delle impostazioni (Bot, agenti, modelli): anche il revisore, che deve
/// scegliere Bot e modello dei task di correzione e della nuova revisione. Sola lettura:
/// `propose_settings` resta della sessione.
const SETTINGS_READERS: &[Role] = &[Role::Session, Role::Reviewer, Role::Planner];
/// Strumenti dell'utente (coda, skill, impostazioni): vietati ai worker.
const SESSION_ONLY: &[Role] = &[Role::Session];
/// Chi puo' fare una domanda bloccante all'utente: il worker e il pianificatore.
/// Il revisore e' escluso di proposito: il suo lavoro e' un verdetto, quindi un
/// dubbio che blocca diventa un rilievo o una revisione fallita e resta non interattivo.
const ASKERS: &[Role] = &[Role::Worker, Role::Planner];

impl Method {
    pub const ALL: [Method; 18] = [
        Method::Whoami,
        Method::GetTask,
        Method::CompleteTask,
        Method::RunE2e,
        Method::GetE2eResult,
        Method::ProposeTasks,
        Method::ListTasks,
        Method::GetTaskDetails,
        Method::UpdateTask,
        Method::DeleteTask,
        Method::SaveSkill,
        Method::ListSkills,
        Method::DeleteSkill,
        Method::GetWorkspaceSettings,
        Method::ProposeSettings,
        Method::AskUser,
        Method::ListGoals,
        Method::GetGoal,
    ];

    /// Nome sul filo.
    pub fn name(self) -> &'static str {
        match self {
            Method::Whoami => "whoami",
            Method::GetTask => "get_task",
            Method::CompleteTask => "complete_task",
            Method::RunE2e => "run_e2e",
            Method::GetE2eResult => "get_e2e_result",
            Method::ProposeTasks => "propose_tasks",
            Method::ListTasks => "list_tasks",
            Method::GetTaskDetails => "get_task_details",
            Method::UpdateTask => "update_task",
            Method::DeleteTask => "delete_task",
            Method::SaveSkill => "save_skill",
            Method::ListSkills => "list_skills",
            Method::DeleteSkill => "delete_skill",
            Method::GetWorkspaceSettings => "get_workspace_settings",
            Method::ProposeSettings => "propose_settings",
            Method::AskUser => "ask_user",
            Method::ListGoals => "list_goals",
            Method::GetGoal => "get_goal",
        }
    }

    pub fn parse(name: &str) -> Option<Method> {
        Method::ALL.into_iter().find(|m| m.name() == name)
    }

    /// I ruoli che possono chiamare il metodo.
    pub fn roles(self) -> &'static [Role] {
        match self {
            Method::Whoami => EVERYONE,
            Method::GetTask | Method::CompleteTask => AUTOMATED,
            Method::AskUser => ASKERS,
            Method::RunE2e | Method::GetE2eResult => E2E_RUNNERS,
            Method::ProposeTasks | Method::GetTaskDetails => PROPOSERS,
            Method::ListTasks | Method::ListGoals => SESSION_AND_PLANNER,
            Method::GetGoal => GOAL_READERS,
            Method::GetWorkspaceSettings => SETTINGS_READERS,
            Method::UpdateTask
            | Method::DeleteTask
            | Method::SaveSkill
            | Method::ListSkills
            | Method::DeleteSkill
            | Method::ProposeSettings => SESSION_ONLY,
        }
    }

    pub fn allows(self, role: Role) -> bool {
        self.roles().contains(&role)
    }

    /// `true` se il metodo esiste solo con l'Agents HUB acceso: l'utente risponde
    /// attraverso l'HUB, senza non c'e' nessuno che possa farlo.
    pub fn requires_hub(self) -> bool {
        match self {
            Method::AskUser => true,
            Method::Whoami
            | Method::GetTask
            | Method::CompleteTask
            | Method::RunE2e
            | Method::GetE2eResult
            | Method::ProposeTasks
            | Method::ListTasks
            | Method::GetTaskDetails
            | Method::UpdateTask
            | Method::DeleteTask
            | Method::SaveSkill
            | Method::ListSkills
            | Method::DeleteSkill
            | Method::GetWorkspaceSettings
            | Method::ProposeSettings
            | Method::ListGoals
            | Method::GetGoal => false,
        }
    }

    /// `true` per l'handshake, che non ha un tool MCP.
    pub fn is_handshake(self) -> bool {
        match self {
            Method::Whoami => true,
            Method::GetTask
            | Method::CompleteTask
            | Method::RunE2e
            | Method::GetE2eResult
            | Method::ProposeTasks
            | Method::ListTasks
            | Method::GetTaskDetails
            | Method::UpdateTask
            | Method::DeleteTask
            | Method::SaveSkill
            | Method::ListSkills
            | Method::DeleteSkill
            | Method::GetWorkspaceSettings
            | Method::ProposeSettings
            | Method::AskUser
            | Method::ListGoals
            | Method::GetGoal => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_method_name_parses_back_to_the_method() {
        for m in Method::ALL {
            assert_eq!(Method::parse(m.name()), Some(m), "{}", m.name());
        }
        assert_eq!(Method::parse("get_events"), None);
        assert_eq!(Method::parse(""), None);
        assert_eq!(Method::parse("GET_TASK"), None);
    }

    #[test]
    fn method_names_are_unique() {
        let names: HashSet<&str> = Method::ALL.iter().map(|m| m.name()).collect();
        assert_eq!(names.len(), Method::ALL.len());
    }

    #[test]
    fn every_role_string_parses_back_to_the_role() {
        for r in Role::ALL {
            assert_eq!(Role::parse(r.as_str()), Some(r), "{}", r.as_str());
        }
        let names: HashSet<&str> = Role::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(names.len(), Role::ALL.len());
        assert_eq!(Role::parse("admin"), None);
        assert_eq!(Role::parse(""), None);
    }

    #[test]
    fn the_role_comes_from_the_kind_of_the_task() {
        assert_eq!(Role::of_worker(None), Role::Session);
        assert_eq!(Role::of_worker(Some(TaskKind::Work)), Role::Worker);
        assert_eq!(Role::of_worker(Some(TaskKind::Review)), Role::Reviewer);
        assert_eq!(Role::of_worker(Some(TaskKind::Plan)), Role::Planner);
    }

    #[test]
    fn whoami_is_the_only_handshake_and_every_role_may_call_it() {
        for r in Role::ALL {
            assert!(Method::Whoami.allows(r), "{}", r.as_str());
        }
        let handshakes: Vec<Method> = Method::ALL.into_iter().filter(|m| m.is_handshake()).collect();
        assert_eq!(handshakes, vec![Method::Whoami]);
    }

    #[test]
    fn allows_agrees_with_the_role_list_and_no_method_is_orphaned() {
        for m in Method::ALL {
            assert!(!m.roles().is_empty(), "{} nobody can call it", m.name());
            for r in Role::ALL {
                assert_eq!(m.allows(r), m.roles().contains(&r), "{} / {}", m.name(), r.as_str());
            }
        }
    }

    #[test]
    fn the_table_keeps_the_agreed_permissions() {
        let who = |m: Method| Role::ALL.into_iter().filter(|r| m.allows(*r)).collect::<Vec<_>>();
        for m in [Method::GetTask, Method::CompleteTask] {
            assert_eq!(who(m), vec![Role::Worker, Role::Reviewer, Role::Planner], "{}", m.name());
        }
        for m in [Method::RunE2e, Method::GetE2eResult] {
            assert_eq!(who(m), vec![Role::Worker, Role::Reviewer], "{}", m.name());
        }
        for m in [Method::ProposeTasks, Method::GetTaskDetails] {
            assert_eq!(who(m), vec![Role::Session, Role::Reviewer, Role::Planner], "{}", m.name());
        }
        assert_eq!(who(Method::ListTasks), vec![Role::Session, Role::Planner]);
        assert_eq!(who(Method::ListGoals), vec![Role::Session, Role::Planner]);
        assert_eq!(who(Method::GetGoal), vec![Role::Session, Role::Reviewer, Role::Planner]);
        assert_eq!(who(Method::GetWorkspaceSettings), vec![Role::Session, Role::Reviewer, Role::Planner]);
        for m in [
            Method::UpdateTask,
            Method::DeleteTask,
            Method::SaveSkill,
            Method::ListSkills,
            Method::DeleteSkill,
            Method::ProposeSettings,
        ] {
            assert_eq!(who(m), vec![Role::Session], "{}", m.name());
        }
        assert_eq!(who(Method::AskUser), vec![Role::Worker, Role::Planner]);
    }

    #[test]
    fn only_ask_user_requires_the_hub() {
        let gated: Vec<Method> = Method::ALL.into_iter().filter(|m| m.requires_hub()).collect();
        assert_eq!(gated, vec![Method::AskUser]);
    }
}
