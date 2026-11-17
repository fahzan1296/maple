//! Pointer actions on transcript blocks in a native terminal (a user message's
//! hover toolbar, an error frame's action row).
//!
//! Reference: `l/src/chat/transcript-actions.ts`.
//!
//! Blocks describe the controls; the interactive host registers the one handler
//! that runs the existing commands for them, so a click and the key binding take
//! the same path.
//!
//! Deviations: the reference's module-global `handler` is a process-wide
//! [`std::sync::RwLock`] slot (the crate's other host hooks use the same
//! pattern), and `TspMenuEntry` from `@oh-my-pi/pi-wire` is modelled here as
//! [`MenuEntry`].

use std::sync::RwLock;

use crate::chat::spotlight::SpotlightDetails;

/// A menu entry a control sheet offers (`TspMenuEntry`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    /// Action id the sheet reports back.
    pub id: String,
    /// Button label.
    pub label: String,
    /// Icon name.
    pub icon: Option<String>,
    /// Secondary line under the label.
    pub detail: Option<String>,
}

impl MenuEntry {
    /// An entry with `id` and `label` and nothing else.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        MenuEntry {
            id: id.into(),
            label: label.into(),
            icon: None,
            detail: None,
        }
    }
}

/// The role of the message a turn action names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnRole {
    /// A user prompt.
    User,
    /// An assistant reply.
    Assistant,
}

impl TurnRole {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            TurnRole::User => "user",
            TurnRole::Assistant => "assistant",
        }
    }
}

/// A transcript message a turn action names: its role and when it was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnMessage {
    /// The message's role.
    pub role: TurnRole,
    /// The message's `timestamp` in milliseconds.
    pub at: i64,
}

/// What a turn's hold menu runs on its message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnAct {
    /// Rewind past the prompt and put it in the editor.
    Edit,
    /// Rewind past the prompt and send it again.
    Retry,
    /// Open the rewind selector on the message.
    Rewind,
    /// Copy the session up to the message into a new one.
    Fork,
}

impl TurnAct {
    /// The hold-menu entry id that runs this action.
    pub fn id(self) -> &'static str {
        match self {
            TurnAct::Edit => "edit-turn",
            TurnAct::Retry => "retry-turn",
            TurnAct::Rewind => "rewind-turn",
            TurnAct::Fork => "fork-turn",
        }
    }

    /// Parse a hold-menu entry id.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "edit-turn" => Some(TurnAct::Edit),
            "retry-turn" => Some(TurnAct::Retry),
            "rewind-turn" => Some(TurnAct::Rewind),
            "fork-turn" => Some(TurnAct::Fork),
            _ => None,
        }
    }
}

/// One action a transcript control runs.
#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptAction {
    /// Retry the last failed turn (`app.retry`).
    Retry,
    /// Open the temporary model picker (`app.model.selectTemporary`).
    SwitchModel,
    /// Open the transcript rewind selector.
    Rewind,
    /// Run a turn's hold-menu action on `message`.
    Turn {
        /// The action.
        turn: TurnAct,
        /// The message it runs on.
        message: TurnMessage,
    },
    /// Resume the session stored at `path`.
    Resume {
        /// Session file path.
        path: String,
    },
    /// Put `text` on the clipboard.
    Copy {
        /// The text.
        text: String,
    },
    /// Open the Spotlight sheet of a shaped tool result.
    Spotlight {
        /// The call that produced the result.
        tool_call_id: String,
        /// The result's shaping trace.
        spotlight: SpotlightDetails,
        /// The text the model saw.
        reduced: String,
    },
}

type Handler = Box<dyn Fn(&TranscriptAction) + Send + Sync>;

static HANDLER: RwLock<Option<Handler>> = RwLock::new(None);

/// Register (or clear, with `None`) the host's handler.
pub fn set_transcript_action_handler(next: Option<Handler>) {
    match HANDLER.write() {
        Ok(mut guard) => *guard = next,
        Err(poisoned) => *poisoned.into_inner() = None,
    }
}

/// Run `action` through the host; false when no host handles actions.
pub fn run_transcript_action(action: TranscriptAction) -> bool {
    let guard = match HANDLER.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    match guard.as_ref() {
        Some(handler) => {
            handler(&action);
            true
        }
        None => false,
    }
}

/// Whether a host handles actions (controls are only described when one does).
pub fn has_transcript_actions() -> bool {
    match HANDLER.read() {
        Ok(guard) => guard.is_some(),
        Err(poisoned) => poisoned.into_inner().is_some(),
    }
}

/// A turn's `actions.hold`: what a long press on it offers on a phone, led by
/// its copy entry, then Edit & resend (a prompt's only), Retry turn, Rewind to
/// here… and Fork from here. A desktop ignores it, so the hover toolbars stay
/// its controls.
pub fn turn_hold_entries(role: TurnRole, copy: MenuEntry) -> Vec<MenuEntry> {
    let mut entries = vec![copy];
    if role == TurnRole::User {
        entries.push(MenuEntry {
            id: TurnAct::Edit.id().to_string(),
            label: "Edit & resend".to_string(),
            icon: Some("edit".to_string()),
            detail: Some("To composer".to_string()),
        });
    }
    entries.push(MenuEntry {
        id: TurnAct::Retry.id().to_string(),
        label: "Retry turn".to_string(),
        icon: Some("refresh".to_string()),
        detail: None,
    });
    entries.push(MenuEntry {
        id: TurnAct::Rewind.id().to_string(),
        label: "Rewind to here…".to_string(),
        icon: Some("rewind".to_string()),
        detail: None,
    });
    entries.push(MenuEntry {
        id: TurnAct::Fork.id().to_string(),
        label: "Fork from here".to_string(),
        icon: Some("branch".to_string()),
        detail: Some("New session".to_string()),
    });
    entries
}

/// Runs hold-menu action `act` on `message`; false when `act` is no turn action
/// or no host handles actions.
pub fn run_turn_action(act: &str, message: TurnMessage) -> bool {
    let Some(turn) = TurnAct::from_id(act) else {
        return false;
    };
    run_transcript_action(TranscriptAction::Turn { turn, message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Serializes tests that touch the process-wide handler.
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        match TEST_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[test]
    fn without_a_handler_actions_report_unhandled() {
        let _guard = test_lock();
        set_transcript_action_handler(None);
        assert!(!has_transcript_actions());
        assert!(!run_transcript_action(TranscriptAction::Retry));
        assert!(!run_turn_action("retry-turn", TurnMessage { role: TurnRole::User, at: 1 }));
    }

    #[test]
    fn a_registered_handler_receives_every_action() {
        let _guard = test_lock();
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        set_transcript_action_handler(Some(Box::new(move |_action| {
            counter.fetch_add(1, Ordering::SeqCst);
        })));
        assert!(has_transcript_actions());
        assert!(run_transcript_action(TranscriptAction::Copy {
            text: "x".into()
        }));
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        set_transcript_action_handler(None);
    }

    #[test]
    fn turn_actions_round_trip_through_their_entry_ids() {
        let cases = [
            ("edit-turn", TurnAct::Edit),
            ("retry-turn", TurnAct::Retry),
            ("rewind-turn", TurnAct::Rewind),
            ("fork-turn", TurnAct::Fork),
        ];
        for (id, act) in cases {
            assert_eq!(TurnAct::from_id(id), Some(act));
            assert_eq!(act.id(), id);
        }
        assert_eq!(TurnAct::from_id("copy-turn"), None);
    }

    #[test]
    fn a_prompt_hold_menu_leads_with_edit_and_a_reply_does_not() {
        let copy = MenuEntry::new("copy", "Copy");
        let prompt: Vec<String> = turn_hold_entries(TurnRole::User, copy.clone())
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(
            prompt.join(","),
            "copy,edit-turn,retry-turn,rewind-turn,fork-turn"
        );
        let reply: Vec<String> = turn_hold_entries(TurnRole::Assistant, copy)
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(reply.join(","), "copy,retry-turn,rewind-turn,fork-turn");
    }

    #[test]
    fn hold_entries_carry_their_labels_icons_and_details() {
        let entries = turn_hold_entries(TurnRole::User, MenuEntry::new("copy", "Copy"));
        let fork = entries.last().expect("fork entry");
        assert_eq!(fork.label, "Fork from here");
        assert_eq!(fork.icon.as_deref(), Some("branch"));
        assert_eq!(fork.detail.as_deref(), Some("New session"));
        let rewind = &entries[3];
        assert_eq!(rewind.label, "Rewind to here…");
        assert_eq!(rewind.detail, None);
    }
}
