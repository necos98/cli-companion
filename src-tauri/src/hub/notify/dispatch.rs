//! La colla con Tauri: controlla le impostazioni, costruisce la notifica, la
//! tiene per la finestra di raggruppamento e poi emette `hub-notify`. Nessun
//! lock resta preso durante l'attesa o l'emissione.

use super::kind::NoticeKind;
use super::notice::{AppMessage, ContactNotice, HubNotice, HUB_NOTIFY_EVENT};
use super::pending::{Candidate, Pending, ThreadKey};
use super::sinks::{dispatch, dispatch_app_message, NoticeSink, Targets};
use super::text;
use crate::hub::channel::Channel;
use crate::hub::model::{Message, ThreadSummary};
use crate::hub::settings::HubSettingsStore;
use crate::hub::text::{clean_line, TITLE_MAX};
use crate::workspaces::WorkspaceStore;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Quanto si aspetta, dal primo candidato di un thread, prima di emettere:
/// un evento seguito dalla risposta del titolare diventa una notifica sola.
const GROUP_WINDOW: Duration = Duration::from_secs(3);

/// Stato Tauri: i candidati in attesa della fine della loro finestra e i
/// destinatari registrati oltre al frontend.
#[derive(Default)]
pub struct HubNotifier {
    pending: Mutex<Pending>,
    sinks: Mutex<Vec<Arc<dyn NoticeSink>>>,
}

impl HubNotifier {
    /// Registra un destinatario delle notifiche oltre al frontend (vedi `sinks`).
    pub fn add_sink(&self, sink: Arc<dyn NoticeSink>) {
        self.lock_sinks().push(sink);
    }

    /// Un candidato per il thread `summary`: `message_id` e `message_text` sono
    /// il messaggio che l'ha causato. Non fa nulla se nessuno riceverebbe la
    /// notifica (HUB spento, o notifiche spente e nessun sink). Veloce: la
    /// finestra e la consegna girano su un thread a parte.
    pub fn notify(
        &self,
        app: &AppHandle,
        summary: &ThreadSummary,
        message_id: &str,
        kind: NoticeKind,
        message_text: &str,
    ) {
        if !targets_of(app, self.has_sinks()).any() {
            return;
        }
        let workspace_name = match &summary.channel {
            Channel::General => None,
            Channel::Workspace(id) => app.state::<WorkspaceStore>().find(id).map(|w| w.name),
        };
        let key = (summary.channel.clone(), summary.id.clone());
        let candidate = candidate(summary, message_id, &kind, message_text, workspace_name.as_deref());
        if !self.lock().offer(key.clone(), candidate) {
            return;
        }
        let app = app.clone();
        // Il progetto non ha un timer async: un thread del pool bloccante che
        // dorme per la finestra.
        tauri::async_runtime::spawn_blocking(move || {
            std::thread::sleep(GROUP_WINDOW);
            app.state::<HubNotifier>().flush(&app, &key);
        });
    }

    /// L'utente ha scritto `message` nel thread `summary` dell'app: lo sanno i sink
    /// (un canale di contatto lo rispecchia). Subito, senza finestra: i messaggi
    /// restano in ordine. Non fa nulla con l'HUB spento o senza sink. Veloce: i sink
    /// mettono in coda e basta.
    pub fn app_message_written(&self, app: &AppHandle, summary: &ThreadSummary, message: &Message) {
        let sinks = self.lock_sinks().clone();
        let targets = targets_of(app, !sinks.is_empty());
        if !targets.sinks {
            return;
        }
        let workspace_name = match &summary.channel {
            Channel::General => None,
            Channel::Workspace(id) => app.state::<WorkspaceStore>().find(id).map(|w| w.name),
        };
        dispatch_app_message(&app_message(summary, message, workspace_name.as_deref()), targets, &sinks);
    }

    /// L'HUB e' stato spento: nessuna notifica in attesa deve uscire.
    pub fn discard(&self) {
        self.lock().clear();
    }

    /// Il thread e' stato eliminato: la sua notifica in attesa non deve uscire
    /// (alla fine della finestra non resta nulla da consegnare).
    pub fn discard_thread(&self, channel: &Channel, thread_id: &str) {
        self.lock().take(&(channel.clone(), thread_id.to_string()));
    }

    /// Chiude la finestra di `key` e consegna quanto resta ai destinatari
    /// ancora attivi. I sink si copiano prima: nessun lock resta preso durante
    /// la consegna.
    fn flush(&self, app: &AppHandle, key: &ThreadKey) {
        let Some(notice) = self.lock().take(key) else {
            return;
        };
        let sinks = self.lock_sinks().clone();
        let targets = targets_of(app, !sinks.is_empty());
        dispatch(&notice, targets, &sinks, |notice| {
            if let Err(e) = app.emit(HUB_NOTIFY_EVENT, notice) {
                log::warn!("hub: notifica non emessa: {e}");
            }
        });
    }

    fn has_sinks(&self) -> bool {
        !self.lock_sinks().is_empty()
    }

    fn lock_sinks(&self) -> MutexGuard<'_, Vec<Arc<dyn NoticeSink>>> {
        self.sinks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn targets_of(app: &AppHandle, has_sinks: bool) -> Targets {
    app.state::<HubSettingsStore>().read(|s| Targets::resolve(s.is_on(), s.notifications, has_sinks))
}

fn candidate(
    summary: &ThreadSummary,
    message_id: &str,
    kind: &NoticeKind,
    message_text: &str,
    workspace_name: Option<&str>,
) -> Candidate {
    let channel_name = text::channel_name(&summary.channel, workspace_name);
    let notice = HubNotice {
        channel: summary.channel.clone(),
        thread_id: summary.id.clone(),
        message_id: message_id.to_string(),
        title: kind.title(),
        body: text::body(&channel_name, &summary.title, message_text),
        sound: kind.sound(),
    };
    let contact = ContactNotice {
        notice,
        category: kind.category(),
        // Il titolo di un thread e' gia' una riga di al piu' TITLE_MAX caratteri.
        thread_title: clean_line(&summary.title, TITLE_MAX),
        channel_name,
        text: message_text.to_string(),
    };
    Candidate { priority: kind.priority(), notice: contact }
}

fn app_message(summary: &ThreadSummary, message: &Message, workspace_name: Option<&str>) -> AppMessage {
    AppMessage {
        channel: summary.channel.clone(),
        thread_id: summary.id.clone(),
        message_id: message.id.clone(),
        channel_name: text::channel_name(&summary.channel, workspace_name),
        thread_title: clean_line(&summary.title, TITLE_MAX),
        text: message.body.clone(),
        attachment_count: message.attachments.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::kind::Priority;
    use super::super::notice::HubNoticeSound;
    use super::*;
    use crate::hub::model::{Attachment, Author, MessageKind, ThreadState};

    fn summary(channel: Channel) -> ThreadSummary {
        ThreadSummary {
            id: "thr-1-1".into(),
            channel,
            title: "Migrazione".into(),
            state: ThreadState::Open,
            task_count: 0,
            message_count: 1,
            waiting_user: false,
            owner: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn a_reply_candidate_carries_title_sound_and_names_instead_of_ids() {
        let s = summary(Channel::Workspace("ws-9-9".into()));
        let c = candidate(&s, "msg-7-1", &NoticeKind::Reply { bot: "ada".into() }, "Fatto.\nAltro", Some("Sito"));
        assert_eq!(c.priority, Priority::Reply);
        assert_eq!(c.notice.notice.title, "@ada ti ha risposto");
        assert_eq!(c.notice.notice.body, "Sito · Migrazione\nFatto.");
        assert_eq!(c.notice.notice.sound, HubNoticeSound::Ding);
        assert_eq!(c.notice.notice.message_id, "msg-7-1");
        assert_eq!(
            (c.notice.notice.channel, c.notice.notice.thread_id.as_str()),
            (Channel::Workspace("ws-9-9".into()), "thr-1-1")
        );
    }

    fn user_message(body: &str, attachments: usize) -> Message {
        let attachment = |n: usize| Attachment {
            id: format!("att-1-{n}"),
            name: "f.txt".into(),
            mime: "text/plain".into(),
            size: 1,
        };
        Message {
            id: "msg-3-1".into(),
            author: Author::User,
            kind: MessageKind::Text,
            body: body.into(),
            reply_to: None,
            event: None,
            attachments: (0..attachments).map(attachment).collect(),
            origin: None,
            created_at: 0,
        }
    }

    #[test]
    fn an_app_message_carries_names_the_full_text_and_the_attachment_count() {
        let s = summary(Channel::Workspace("ws-9-9".into()));
        let m = app_message(
            &s,
            &user_message(
                "Prima riga
Seconda riga",
                2,
            ),
            Some("Sito"),
        );
        assert_eq!(
            (m.channel, m.thread_id.as_str(), m.message_id.as_str()),
            (Channel::Workspace("ws-9-9".into()), "thr-1-1", "msg-3-1")
        );
        assert_eq!((m.channel_name.as_str(), m.thread_title.as_str()), ("Sito", "Migrazione"));
        assert_eq!(
            (m.text.as_str(), m.attachment_count),
            (
                "Prima riga
Seconda riga",
                2
            )
        );
    }

    #[test]
    fn an_app_message_of_a_removed_workspace_never_shows_its_id() {
        let s = summary(Channel::Workspace("ws-9-9".into()));
        assert_eq!(app_message(&s, &user_message("x", 0), None).channel_name, "Workspace");
        assert_eq!(app_message(&summary(Channel::General), &user_message("x", 0), None).channel_name, "Generale");
    }

    #[test]
    fn the_general_channel_and_a_removed_workspace_have_fixed_names() {
        let general = candidate(&summary(Channel::General), "m", &NoticeKind::TaskFailed, "", None);
        assert_eq!(general.notice.notice.body, "Generale · Migrazione");
        assert_eq!(general.notice.notice.sound, HubNoticeSound::Attention);
        let gone = candidate(&summary(Channel::Workspace("ws-9-9".into())), "m", &NoticeKind::TaskDone, "ok", None);
        assert_eq!(gone.notice.notice.body, "Workspace · Migrazione\nok");
        assert!(!gone.notice.notice.body.contains("ws-9-9"));
    }

    #[test]
    fn discarding_drops_the_pending_notices() {
        let notifier = HubNotifier::default();
        let s = summary(Channel::General);
        let key = (s.channel.clone(), s.id.clone());
        notifier.lock().offer(key.clone(), candidate(&s, "m", &NoticeKind::TaskDone, "x", None));
        notifier.discard();
        assert!(notifier.lock().take(&key).is_none());
    }

    #[test]
    fn discarding_a_thread_drops_its_pending_notice_only() {
        let notifier = HubNotifier::default();
        let gone = summary(Channel::General);
        let kept = ThreadSummary { id: "thr-2-2".into(), ..summary(Channel::General) };
        for s in [&gone, &kept] {
            notifier
                .lock()
                .offer((s.channel.clone(), s.id.clone()), candidate(s, "m", &NoticeKind::TaskDone, "x", None));
        }
        notifier.discard_thread(&gone.channel, &gone.id);
        assert!(notifier.lock().take(&(gone.channel.clone(), gone.id.clone())).is_none());
        assert!(notifier.lock().take(&(kept.channel.clone(), kept.id.clone())).is_some());
    }
}
