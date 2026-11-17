//! Port of `l/test/agent-hub-native.test.ts`.
//!
//! The reference drives native picker select/action events and asserts picker
//! `facts.time` aging. The crate hub has keyboard selection, tree view toggle,
//! and a Rows native describe, but no `handle_native_event`, picker props, or
//! observer time facts, so those scenarios are ignored; keyboard selection,
//! Enter focus, tree toggle, and Rows describe are asserted.

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NodeKind};
use maple_tui::overlays::agent_hub::{AgentHubCallbacks, AgentHubOverlayComponent};
use maple_tui::overlays::agent_hub_types::{
    AgentHubRegistry, AgentLifecycleLike, AgentRecordLike, AgentStatus, IrcBusLike,
    LivePromptHandle,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
struct Rec {
    id: String,
    status: AgentStatus,
    kind: String,
    parent: Option<String>,
    last_activity: i64,
}

impl AgentRecordLike for Rec {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> &str {
        &self.kind
    }
    fn parent_id(&self) -> Option<&str> {
        self.parent.as_deref()
    }
    fn status(&self) -> AgentStatus {
        self.status
    }
    fn session(&self) -> Option<Arc<dyn maple_tui::overlays::agent_hub_types::AgentHubSession + Send + Sync>> {
        None
    }
    fn session_file(&self) -> Option<&str> {
        None
    }
    fn created_at(&self) -> i64 {
        0
    }
    fn last_activity(&self) -> i64 {
        self.last_activity
    }
    fn activity(&self) -> Option<&str> {
        None
    }
    fn history(&self) -> Option<maple_tui::overlays::agent_hub_types::AgentRecordHistory> {
        None
    }
    fn lifecycle(&self) -> Option<maple_tui::overlays::agent_hub_types::AgentRecordLifecycle> {
        None
    }
}

struct Reg {
    list: Vec<Arc<Rec>>,
}

impl AgentHubRegistry for Reg {
    type Record = Rec;
    fn list(&self) -> Vec<Arc<Self::Record>> {
        self.list.clone()
    }
    fn get(&self, id: &str) -> Option<Arc<Self::Record>> {
        self.list.iter().find(|r| r.id == id).cloned()
    }
    fn on_change(&self, _listener: Box<dyn Fn() + Send + Sync>) -> Box<dyn FnOnce() + Send> {
        Box::new(|| {})
    }
}

struct Prompt;

impl LivePromptHandle for Prompt {
    fn prompt(&self, _text: &str, _streaming_behavior: &str) -> Result<(), String> {
        Ok(())
    }
}

struct Life;

impl AgentLifecycleLike<Reg> for Life {
    fn ensure_live(&self, _id: &str) -> Result<Arc<dyn LivePromptHandle + Send + Sync>, String> {
        Ok(Arc::new(Prompt))
    }
    fn release(&self, _id: &str, _expected: &Rec, _tombstone: bool) -> Result<bool, String> {
        Ok(true)
    }
}

struct Irc;

impl IrcBusLike for Irc {
    fn unread_count(&self, _id: &str) -> usize {
        0
    }
}

#[derive(Clone)]
struct Cb {
    focused: Rc<RefCell<Vec<String>>>,
    done: Rc<RefCell<bool>>,
}

impl AgentHubCallbacks for Cb {
    fn on_done(&self) {
        *self.done.borrow_mut() = true;
    }
    fn request_render(&self) {}
    fn focus_agent(&self, id: &str) -> Result<(), String> {
        self.focused.borrow_mut().push(id.to_string());
        Ok(())
    }
}

fn agent(id: &str, last_activity: i64) -> Arc<Rec> {
    Arc::new(Rec {
        id: id.to_string(),
        status: AgentStatus::Idle,
        kind: "worker".to_string(),
        parent: None,
        last_activity,
    })
}

fn hub(agents: Vec<Arc<Rec>>, cb: Cb) -> AgentHubOverlayComponent<Reg, Life, Irc, Cb> {
    AgentHubOverlayComponent::new(Arc::new(Reg { list: agents }), Arc::new(Life), Arc::new(Irc), cb)
}

fn cx() -> DescribeContext {
    DescribeContext { cols: 120, ..DescribeContext::default() }
}

#[test]
fn down_then_enter_focuses_selected_agent() {
    let cb = Cb {
        focused: Rc::new(RefCell::new(Vec::new())),
        done: Rc::new(RefCell::new(false)),
    };
    let focused = cb.focused.clone();
    let done = cb.done.clone();
    let mut hub = hub(
        vec![agent("alpha", 3_000), agent("beta", 2_000), agent("gamma", 1_000)],
        cb,
    );
    hub.handle_input(&Key::Down);
    hub.handle_input(&Key::Enter);
    assert_eq!(*focused.borrow(), vec!["beta".to_string()]);
    assert!(*done.borrow());
}

#[test]
fn t_toggles_tree_view_mode() {
    let mut hub = hub(vec![agent("alpha", 3_000)], Cb {
        focused: Rc::new(RefCell::new(Vec::new())),
        done: Rc::new(RefCell::new(false)),
    });
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("viewMode"), Some("Roster"));
    hub.handle_input(&Key::Char('t'));
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("viewMode"), Some("Tree"));
    hub.handle_input(&Key::Char('t'));
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("viewMode"), Some("Roster"));
}

#[test]
fn describe_native_reports_selected_id_and_rows() {
    let mut hub = hub(
        vec![agent("alpha", 3_000), agent("beta", 2_000)],
        Cb {
            focused: Rc::new(RefCell::new(Vec::new())),
            done: Rc::new(RefCell::new(false)),
        },
    );
    let node = hub.describe_native(&cx());
    assert!(matches!(node.kind, NodeKind::Rows));
    assert_eq!(node.props.get_str("selectedId"), Some("alpha"));
    hub.handle_input(&Key::Down);
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("selectedId"), Some("beta"));
}

#[test]
#[ignore = "missing facility: handle_native_event picker select/action routing"]
fn picker_select_and_open_action_take_enter_path() {
    let mut hub = hub(vec![agent("alpha", 3_000)], Cb {
        focused: Rc::new(RefCell::new(Vec::new())),
        done: Rc::new(RefCell::new(false)),
    });
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("selectedId"), Some("alpha"));
}

#[test]
#[ignore = "missing facility: picker parent-tree items with depth"]
fn by_parent_action_and_t_key_switch_to_parent_tree() {
    let mut hub = hub(vec![agent("alpha", 3_000)], Cb {
        focused: Rc::new(RefCell::new(Vec::new())),
        done: Rc::new(RefCell::new(false)),
    });
    hub.handle_input(&Key::Char('t'));
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("viewMode"), Some("Tree"));
}

#[test]
#[ignore = "missing facility: picker facts.time from observer progress frames"]
fn running_row_time_is_age_at_send() {
    let mut hub = hub(vec![agent("Worker", 1_000_000)], Cb {
        focused: Rc::new(RefCell::new(Vec::new())),
        done: Rc::new(RefCell::new(false)),
    });
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("selectedId"), Some("Worker"));
}

#[test]
#[ignore = "missing facility: reconciler item diffing on selection change"]
fn selection_change_resends_no_roster_rows() {
    let mut hub = hub(vec![agent("Worker", 1_000_000), agent("Idle", 900_000)], Cb {
        focused: Rc::new(RefCell::new(Vec::new())),
        done: Rc::new(RefCell::new(false)),
    });
    hub.handle_input(&Key::Down);
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("selectedId"), Some("Idle"));
}

#[test]
#[ignore = "missing facility: lifecycle snapshot aging across idle gaps"]
fn follow_up_turn_does_not_age_previous_snapshot() {
    let mut hub = hub(vec![agent("Worker", 1_000_000)], Cb {
        focused: Rc::new(RefCell::new(Vec::new())),
        done: Rc::new(RefCell::new(false)),
    });
    hub.handle_input(&Key::Down);
    let node = hub.describe_native(&cx());
    assert_eq!(node.props.get_str("selectedId"), Some("Worker"));
}
