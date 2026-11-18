//! Nucleo puro dell'automazione: `Registry` (worker e board), gli effetti da
//! eseguire fuori dal lock e le operazioni di base sui worker.

use super::*;
use ts_rs::TS;

/// Worker (anche chiusi) tenuti in memoria prima di scartare i piu' vecchi.
pub(super) const MAX_TRACKED_WORKERS: usize = 256;

/// Il task non e' piu' assegnato alla sessione (lo legge il modello).
pub(crate) const TASK_UNASSIGNED: &str = "this task is no longer assigned to this session";

/// Notifica di sistema per l'utente (evento `task-notify`): un task richiede
/// attenzione o e' fallito. Il testo e' in italiano.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Notice {
    pub workspace_id: String,
    pub task_id: String,
    /// Tab del worker, se esiste ancora (il toast cliccabile porta li').
    pub tab_id: Option<String>,
    /// `attention` | `failed` | `question`
    pub kind: &'static str,
    /// Titolo del task.
    pub title: String,
    pub body: String,
}

/// Effetto da eseguire fuori dal lock, accumulato dal `Registry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Update(AutomationTabInfo),
    Close {
        tab_id: String,
        delay: Duration,
    },
    /// Avviare un task di board (git + spawn) su un thread separato.
    StartTask(Box<StartJob>),
    /// Task conclusi usciti dalla board (`Board::take_archivable`): da
    /// aggiungere all'archivio. Sempre prima del `Publish` che salva la board
    /// senza di loro.
    Archive {
        ws_id: String,
        tasks: Vec<Task>,
    },
    /// Salvare su disco e notificare al frontend la board (`board-update`).
    Publish(Box<Board>, Box<BoardView>),
    /// La board del workspace e' stata rimossa: cancellare il file. `rev` e'
    /// una revisione di rimozione: salvataggi piu' vecchi in ritardo vengono
    /// scartati (niente file resuscitato).
    DeleteBoard(String, u64),
    /// Rimuovere worktree e branch di un task concluso o eliminato.
    Cleanup(Box<CleanupTarget>),
    Notify(Box<Notice>),
    /// Prendere (`on`) o rilasciare il blocco dello standby. `seq` ordina gli
    /// effetti applicati fuori dal lock da thread diversi: uno vecchio in
    /// ritardo viene scartato.
    KeepAwake {
        on: bool,
        seq: u64,
    },
    /// Emettere `schedule-state`.
    Schedule(Box<crate::schedule::ScheduleState>),
    /// Annullare le esecuzioni E2E del task (o, senza task, di tutto il workspace).
    CancelE2e {
        ws_id: String,
        task_id: Option<String>,
    },
    /// Confrontare lo stato dei repository di un pianificatore alla root con
    /// quello dell'avvio (git: su un thread).
    ReadOnlyCheck(Box<planner::root::ReadOnlyCheck>),
}

/// Cio' che `complete_task` porta oltre stato e riassunto: i campi del report e
/// i dati calcolati fuori dal lock (diffstat git, blocco E2E).
pub struct Completion {
    pub fields: ReportFields,
    pub diffstat: Option<String>,
    pub gate: Option<e2e::E2eGateInput>,
}

#[derive(Default)]
pub struct Registry {
    pub(super) workers: HashMap<String, Worker>,
    pub(super) effects: Vec<Effect>,
    /// Board per workspace (vedi `boards`).
    pub(super) boards: HashMap<String, Board>,
    /// Workspace con board modificata da schedulare/salvare/notificare.
    pub(super) dirty: Vec<String>,
    /// Ultima revisione assegnata alle board (contatore globale monotono:
    /// resta crescente anche se una board sparisce e ricompare).
    pub(super) next_rev: u64,
    next_worker: u64,
    pub(super) next_proposal: u64,
    /// Fasce orarie e standby (vedi `auto`).
    pub(super) auto: auto::AutoCore,
    /// Agenti non pronti per workspace, solo se ce n'e' qualcuno (vedi
    /// `boards::agents`).
    pub(super) gates: HashMap<String, AgentGate>,
}

impl Registry {
    /// Tipo del task del worker `session` (`None` se non e' un worker).
    pub fn worker_kind(&self, session: &str) -> Option<TaskKind> {
        self.workers.get(session).map(|w| w.kind)
    }

    /// Effetti accumulati dall'ultima chiamata.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Stato del worker: se e' finito quello, altrimenti l'attivita' che il suo
    /// task gli attribuisce; se il task non e' piu' in corso con questo worker
    /// (ripreso, archiviato) il worker e' staccato: `closed`.
    pub(super) fn worker_status(&self, w: &Worker) -> WorkerStatus {
        if let Some(end) = w.ended {
            return end.into();
        }
        let task = self.boards.get(&w.workspace_id).and_then(|b| b.task(&w.task_id));
        task.and_then(|t| t.live_activity(&w.session)).map_or(WorkerStatus::Closed, WorkerStatus::from)
    }

    pub(super) fn worker_status_of(&self, session: &str) -> Option<WorkerStatus> {
        self.workers.get(session).map(|w| self.worker_status(w))
    }

    /// L'attivita' del worker, se non e' finito.
    pub(super) fn worker_activity(&self, session: &str) -> Option<WorkerActivity> {
        self.worker_status_of(session).and_then(WorkerStatus::activity)
    }

    /// Id per il prossimo worker.
    pub(super) fn next_worker_id(&mut self, now: i64) -> String {
        self.next_worker += 1;
        format!("auto-{now}-{}", self.next_worker)
    }

    /// Registra il worker `id` del task `tid` (tentativo `attempt`) del workspace
    /// di `new`; il task lo ha gia' spawnato.
    pub(super) fn spawn_worker(
        &mut self,
        id: String,
        tid: &str,
        attempt: u32,
        new: NewWorker,
        env: WorkerEnv,
        now: i64,
    ) -> SpawnEvent {
        let ws = new.workspace_id.clone();
        let ev = SpawnEvent {
            tab_id: id.clone(),
            workspace_id: new.workspace_id,
            command: new.command,
            cwd: Some(new.cwd),
            title: new.title.clone(),
            parent_tab_id: String::new(),
            agent_id: new.agent_id,
        };
        self.workers.insert(
            id.clone(),
            Worker {
                session: id,
                workspace_id: ws,
                task_id: tid.to_string(),
                kind: new.kind,
                attempt,
                title: new.title,
                env,
                ended: None,
                agent: agent_session::AgentSession::default(),
                created_at: now,
                updated_at: now,
                last_report: None,
                started_sent: false,
                tab_open: true,
                cleanup_on_close: false,
                proposed: false,
            },
        );
        self.prune();
        ev
    }

    /// Scarta i worker chiusi piu' vecchi oltre il tetto di memoria.
    fn prune(&mut self) {
        while self.workers.len() > MAX_TRACKED_WORKERS {
            let oldest =
                self.workers.values().filter(|w| !w.tab_open).min_by_key(|w| w.created_at).map(|w| w.session.clone());
            match oldest {
                Some(id) => {
                    self.workers.remove(&id);
                }
                None => break,
            }
        }
    }

    fn worker_mut(&mut self, session: &str) -> Result<&mut Worker, String> {
        self.workers.get_mut(session).ok_or_else(|| "reserved for automated sessions".to_string())
    }

    /// Worker vivo (tab aperto, non chiuso) del chiamante.
    pub(super) fn live_worker(&mut self, session: &str) -> Result<&mut Worker, String> {
        let closed = self.worker_status_of(session) == Some(WorkerStatus::Closed);
        let w = self.worker_mut(session)?;
        if closed || !w.tab_open {
            return Err("the session is closed".to_string());
        }
        Ok(w)
    }

    /// Pubblica lo stato del worker al frontend.
    pub(super) fn touch(&mut self, session: &str, now: i64) {
        let Some(status) = self.worker_status_of(session) else {
            return;
        };
        if let Some(w) = self.workers.get_mut(session) {
            w.updated_at = now;
            self.effects.push(Effect::Update(w.info(status)));
        }
    }

    /// Notifica di sistema per il task del worker (`attention` | `failed` | `question`).
    pub(super) fn notify(&mut self, ws: &str, tid: &str, tab: Option<&str>, kind: &'static str) {
        let Some(t) = self.boards.get(ws).and_then(|b| b.task(tid)) else {
            return;
        };
        let body = match kind {
            "attention" => format!("{} richiede attenzione", t.title),
            "question" => format!("{} ha una domanda", t.title),
            _ => format!("{} è fallito", t.title),
        };
        let notice = Notice {
            workspace_id: ws.to_string(),
            task_id: tid.to_string(),
            tab_id: tab.map(str::to_string),
            kind,
            title: t.title.clone(),
            body,
        };
        self.effects.push(Effect::Notify(Box::new(notice)));
    }

    // --- worker ---

    /// `get_task`: istruzioni, ambiente e passi di consegna del task. La prima
    /// volta il worker passa a `running`.
    pub fn get_task(&mut self, session: &str, ws: &Workspace, ask_user: bool, now: i64) -> Result<Value, String> {
        let w = self.live_worker(session)?;
        let (ws_id, tid, env) = (w.workspace_id.clone(), w.task_id.clone(), w.env.clone());
        let unassigned = || TASK_UNASSIGNED.to_string();
        let b = self.boards.get(&ws_id).ok_or_else(unassigned)?;
        let t = b.task(&tid).filter(|t| t.is_live_for(session)).ok_or_else(unassigned)?;
        let info = board::texts::EnvInfo {
            delivery: &env.delivery,
            root: &ws.path,
            repo_dir: &env.repo_dir,
            workdir: &env.workdir,
            task: t,
            branch: env.branch.as_deref(),
            base: env.base.as_deref(),
            repo_cfg: ws.repos.get(&board::repo_key(t.repo.as_deref())),
            root_repos: env.root_plan.as_ref().map(|p| p.repos.as_slice()),
            free: env.free,
        };
        let mut out = match t.kind {
            TaskKind::Review => review::get_task_json(&info, &ws.worker.rules, b),
            TaskKind::Plan => planner::get_task_json(&info, &ws.worker.rules, ask_user),
            TaskKind::Work => serde_json::json!({
                "task": t.task,
                "title": t.title,
                "instructions": board::texts::instructions(&info, &ws.worker.rules, ask_user),
                "environment": board::texts::environment(&info),
                "delivery": board::texts::delivery_steps(&info),
            }),
        };
        task_skill::add_to_task_json(&mut out, t);
        if let Some(p) = t.previous_attempt() {
            out["previous_attempt"] = serde_json::to_value(p).unwrap_or(Value::Null);
        }
        let w = self.worker_mut(session)?;
        let first = !w.started_sent;
        w.started_sent = true;
        let read = self.worker_activity(session).and_then(|a| a.on(WorkerSignal::TaskRead).to);
        match read {
            Some(to) => self.set_activity(session, to, now),
            None if first => self.touch(session, now),
            None => {}
        }
        Ok(out)
    }

    /// `complete_task`: il task diventa `done` (consegnato) o `failed` con il
    /// report; il tab si chiude dopo `CLOSE_DELAY`. Un task fallito per un
    /// `session-end` mentre il tab era aperto viene prima riaperto. Un task
    /// marcato E2E si completa solo con un'esecuzione passata sul codice
    /// consegnato (`gate`, vedi `e2e`).
    pub fn complete_task(
        &mut self,
        session: &str,
        status: &str,
        summary: &str,
        extra: Completion,
        now: i64,
    ) -> Result<(), String> {
        // `blocked` e' un sinonimo di `failed`.
        let completed = match status {
            "completed" => true,
            "failed" | "blocked" => false,
            _ => return Err("invalid status (use completed or failed)".to_string()),
        };
        let summary = req_text("summary", summary, SUMMARY_MAX)?;
        // Il verdetto si valida prima di ogni effetto: un errore lascia il task in corso.
        let kind = self.worker_kind(session).ok_or_else(|| "reserved for automated sessions".to_string())?;
        board::check_verdict(kind, completed, &extra.fields)?;
        board::check_plan_report(kind, completed, &extra.fields)?;
        let gate_outcome = self.e2e_gate(session, completed, extra.gate.as_ref())?;
        self.reopen_after_session_end(session, now);
        let w = self.live_worker(session)?;
        let (ws, tid) = (w.workspace_id.clone(), w.task_id.clone());
        w.ended = Some(if completed { WorkerEnd::Completed } else { WorkerEnd::Failed });
        w.last_report = Some(clip_chars(&summary, 200));
        let outcome = if completed {
            Outcome::Done
        } else {
            Outcome::Failed(Failure { cause: FailCause::Other, reason: FailReason::WorkerFailed })
        };
        let mut report =
            build_report(if completed { "completed" } else { "failed" }, &summary, extra.fields, extra.diffstat);
        report.e2e = self.last_e2e_run(&ws, &tid);
        let note =
            (gate_outcome == board::GateOutcome::ConfigRemoved).then(|| board::E2E_GATE_SKIPPED_NOTE.to_string());
        let finished = self
            .boards
            .get_mut(&ws)
            .is_some_and(|b| b.finish_running(&tid, Some(session), outcome, Some(report), note, now));
        if finished {
            self.cancel_e2e(&ws, Some(&tid));
            let has_worktree = self.boards.get(&ws).and_then(|b| b.task(&tid)).is_some_and(|t| t.worktree.is_some());
            if let Some(w) = self.workers.get_mut(session) {
                w.cleanup_on_close = completed && has_worktree;
            }
            self.mark_dirty(&ws);
            if !completed {
                self.notify(&ws, &tid, Some(session), "failed");
            }
        }
        self.touch(session, now);
        self.effects.push(Effect::Close { tab_id: session.to_string(), delay: CLOSE_DELAY });
        Ok(())
    }

    /// Task `running` -> `failed` con una nota; `true` se era ancora in corso.
    pub(super) fn fail_task(
        &mut self,
        ws: &str,
        tid: &str,
        session: Option<&str>,
        note: &str,
        failure: Failure,
        now: i64,
    ) -> bool {
        let outcome = Outcome::Failed(failure);
        let done = self
            .boards
            .get_mut(ws)
            .is_some_and(|b| b.finish_running(tid, session, outcome, None, Some(note.to_string()), now));
        if done {
            self.mark_dirty(ws);
            self.cancel_e2e(ws, Some(tid));
        }
        done
    }

    /// Worker con tab aperto (per ricostruire lo stato del frontend).
    pub fn open_tabs(&self) -> Vec<AutomationTabInfo> {
        let mut v: Vec<&Worker> = self.workers.values().filter(|w| w.tab_open).collect();
        v.sort_by_key(|w| w.created_at);
        v.into_iter().map(|w| w.info(self.worker_status(w))).collect()
    }
}
