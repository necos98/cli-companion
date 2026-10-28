//! Stato Tauri dell'automazione: il registro dietro un mutex, la pubblicazione
//! ordinata delle board (evento + salvataggio coalescente) e l'applicazione
//! degli effetti fuori dal lock.

use super::*;

/// Salvataggi coalescenti: al massimo uno per board ogni `SAVE_EVERY_MS`,
/// sempre con l'ultimo stato (le modifiche ravvicinate non riscrivono il JSON
/// intero ogni volta). L'evento `board-update` non e' coalescente.
pub(super) const SAVE_EVERY_MS: i64 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SaveDecision {
    /// Salvare subito.
    Now,
    /// Un salvataggio e' in coda: armare un timer fra `Duration`.
    Arm(Duration),
    /// Gia' in coda con un timer armato: lo stato nuovo lo sostituisce.
    Queued,
}

#[derive(Default)]
struct SaveSlot {
    last_at: Option<i64>,
    pending: Option<Board>,
    armed: bool,
}

/// Parte pura della coalescenza (tempo iniettato).
#[derive(Default)]
pub(super) struct SaveGate {
    slots: HashMap<String, SaveSlot>,
}

impl SaveGate {
    pub(super) fn offer(&mut self, board: &Board, now: i64) -> SaveDecision {
        let s = self.slots.entry(board.workspace_id.clone()).or_default();
        let since = s.last_at.map_or(i64::MAX, |t| now - t);
        if s.pending.is_none() && !s.armed && since >= SAVE_EVERY_MS {
            s.last_at = Some(now);
            return SaveDecision::Now;
        }
        s.pending = Some(board.clone());
        if s.armed {
            SaveDecision::Queued
        } else {
            s.armed = true;
            SaveDecision::Arm(Duration::from_millis((SAVE_EVERY_MS - since).clamp(0, SAVE_EVERY_MS) as u64))
        }
    }

    /// Il timer e' scattato: l'ultimo stato da salvare, se c'e'.
    pub(super) fn take(&mut self, ws: &str, now: i64) -> Option<Board> {
        let s = self.slots.get_mut(ws)?;
        s.armed = false;
        s.last_at = Some(now);
        s.pending.take()
    }

    pub(super) fn take_all(&mut self) -> Vec<Board> {
        self.slots.values_mut().filter_map(|s| s.pending.take()).collect()
    }

    pub(super) fn forget(&mut self, ws: &str) {
        self.slots.remove(ws);
    }
}

/// Managed state Tauri. `Clone` (Arc): i thread di avvio dei task lo portano
/// con se' per rientrare dopo il lavoro git.
#[derive(Default, Clone)]
pub struct AutomationState {
    pub(super) inner: Arc<Mutex<Registry>>,
    /// Ultima revisione pubblicata (emessa) per board: una pubblicazione in
    /// ritardo non ne sovrascrive una piu' recente. Alla rimozione di una
    /// board ci resta la revisione di rimozione (lapide). Ordine dei lock:
    /// `published` poi `saves`; mai con `inner` in mano.
    published: Arc<Mutex<HashMap<String, u64>>>,
    saves: Arc<Mutex<SaveGate>>,
    /// Ultimo `seq` di `KeepAwake` applicato.
    awake_applied: Arc<Mutex<u64>>,
    /// Un avvio alla volta per task (git worktree sullo stesso path).
    pub(super) starts: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Cache dei `git fetch` (60 s per repository e base): un fetch alla volta.
    pub(super) fetches: Arc<Mutex<worktrees::FetchCache>>,
    /// Una decisione alla volta sulle proposte di impostazioni: applicarne una
    /// due volte (doppio clic) duplicherebbe gli avvii senza `id`.
    pub(super) settings_decisions: Arc<Mutex<()>>,
}

impl AutomationState {
    /// Esegue `f` sul registro, poi schedula le board modificate e applica
    /// gli effetti fuori dal lock.
    pub fn run<H: Host, T>(&self, host: &H, f: impl FnOnce(&mut Registry) -> T) -> T {
        let (out, effects) = {
            let mut reg = self.inner.lock().unwrap();
            let out = f(&mut reg);
            let now = now_millis();
            reg.flush_boards(now, host.local_minute(now), &|id| host.workspace(id));
            (out, reg.take_effects())
        };
        self.apply_effects(host, effects);
        out
    }

    pub fn load_board(&self, board: Board) {
        self.inner.lock().unwrap().load_board(board);
    }

    pub fn open_tabs(&self) -> Vec<AutomationTabInfo> {
        self.inner.lock().unwrap().open_tabs()
    }

    /// Vista della board del workspace (vuota se non esiste).
    pub fn view<H: Host>(&self, host: &H, ws_id: &str) -> Result<BoardView, String> {
        let ws = host.workspace(ws_id).ok_or_else(|| "workspace non trovato".to_string())?;
        let minute = host.local_minute(now_millis());
        Ok(self.inner.lock().unwrap().board_view(&ws, minute))
    }

    fn save<H: Host>(host: &H, board: &Board) {
        if let Err(e) = host.save_board(board) {
            eprintln!("board {}: {e}", board.workspace_id);
        }
    }

    /// Emette la vista se la board e' piu' recente dell'ultima pubblicata e la
    /// salva (coalescendo i salvataggi ravvicinati).
    fn publish<H: Host>(&self, host: &H, board: &Board, view: &BoardView) {
        let decision = {
            let mut map = self.published.lock().unwrap();
            if board.rev <= map.get(&board.workspace_id).copied().unwrap_or(0) {
                return;
            }
            map.insert(board.workspace_id.clone(), board.rev);
            host.emit_board(view);
            let d = self.saves.lock().unwrap().offer(board, now_millis());
            if d == SaveDecision::Now {
                Self::save(host, board);
            }
            d
        };
        // Il timer fuori dal lock: un host sincrono lo eseguirebbe subito.
        if let SaveDecision::Arm(delay) = decision {
            let (st, h, ws) = (self.clone(), host.clone(), board.workspace_id.clone());
            host.defer(delay, Box::new(move || st.flush_save(&h, &ws)));
        }
    }

    /// Timer di salvataggio scattato.
    fn flush_save<H: Host>(&self, host: &H, ws: &str) {
        let map = self.published.lock().unwrap();
        let Some(b) = self.saves.lock().unwrap().take(ws, now_millis()) else {
            return;
        };
        // Board rimossa nel frattempo (lapide): niente file resuscitato.
        if map.get(ws) == Some(&b.rev) {
            Self::save(host, &b);
        }
    }

    /// Chiusura dell'app: salva i salvataggi ancora in coda.
    pub fn flush_pending_saves<H: Host>(&self, host: &H) {
        let map = self.published.lock().unwrap();
        for b in self.saves.lock().unwrap().take_all() {
            if map.get(&b.workspace_id) == Some(&b.rev) {
                Self::save(host, &b);
            }
        }
    }

    /// Il workspace e' stato rimosso: via la sua board e il file.
    pub fn remove_board<H: Host>(&self, host: &H, ws_id: &str) {
        self.run(host, |r| r.remove_board(ws_id));
    }

    fn apply_effects<H: Host>(&self, host: &H, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::StartTask(job) => {
                    // Git e' lento: mai sul thread corrente (puo' essere il main).
                    let (st, h) = (self.clone(), host.clone());
                    host.defer(Duration::ZERO, Box::new(move || worktrees::run_start_job(&st, &h, *job)));
                }
                Effect::Cleanup(target) => {
                    let (st, h) = (self.clone(), host.clone());
                    host.defer(CLEANUP_DELAY, Box::new(move || worktrees::run_cleanup(&st, &h, *target)));
                }
                Effect::Publish(board, view) => self.publish(host, &board, &view),
                Effect::DeleteBoard(ws_id, rev) => {
                    {
                        let mut map = self.published.lock().unwrap();
                        self.saves.lock().unwrap().forget(&ws_id);
                        map.insert(ws_id.clone(), rev);
                        host.delete_board(&ws_id);
                    }
                    let mut view = Board::new(&ws_id).view_with(None, false);
                    view.rev = rev;
                    host.emit_board(&view);
                }
                Effect::Update(info) => host.emit_update(&info),
                Effect::Close { tab_id, delay } if delay.is_zero() => host.emit_close(&tab_id),
                Effect::Close { tab_id, delay } => {
                    let h = host.clone();
                    host.defer(delay, Box::new(move || h.emit_close(&tab_id)));
                }
                Effect::Notify(notice) => host.emit_notice(&notice),
                Effect::KeepAwake { on, seq } => {
                    // Un effetto vecchio, arrivato dopo uno piu' nuovo, si scarta.
                    let mut last = self.awake_applied.lock().unwrap();
                    if seq > *last {
                        *last = seq;
                        host.set_keep_awake(on);
                    }
                }
                Effect::Schedule(state) => host.emit_schedule(&state),
            }
        }
    }
}
