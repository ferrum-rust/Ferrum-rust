//! Selection only. The Renderer owns buffers and performs checked transitions.
use super::renderer_arena::Choice;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Selection { Hold, Original, Arena(Choice) }

pub(super) struct Policy {
    choice: Option<Choice>,
    distance: u32,
    supported: bool,
    failure: Option<String>,
    diagnostic: Option<&'static str>,
}
impl Policy {
    pub(super) fn new(choice: Option<Choice>, distance: u32, supported: bool) -> Self {
        Self { choice, distance, supported, failure: None, diagnostic: None }
    }
    pub(super) fn automatic(&self) -> bool { self.choice.is_none() }
    pub(super) fn distance(&self) -> u32 { self.distance }
    pub(super) fn update_distance(&mut self, distance: u32) { self.distance = distance; }
    pub(super) fn desired(&self) -> Option<Choice> {
        match self.choice {
            Some(Choice::Original) => None,
            Some(choice) => Some(choice),
            None => (self.distance == 128 && self.supported && self.failure.is_none()
                && self.diagnostic.is_none()).then_some(Choice::Mdi),
        }
    }
    pub(super) fn select(&self, active: bool, empty: bool) -> Selection {
        match (self.desired(), active, empty) {
            (Some(_), true, _) | (None, true, false) => Selection::Hold,
            (Some(choice), false, _) => Selection::Arena(choice),
            (None, _, _) => Selection::Original,
        }
    }
    pub(super) fn initialization_failed(&mut self, reason: &str) {
        if self.automatic() && self.failure.is_none() {
            self.failure = Some(reason.chars().take(512).collect());
        }
    }
    pub(super) fn suppress_diagnostic(&mut self, reason: &'static str) {
        if self.automatic() && self.diagnostic.is_none() { self.diagnostic = Some(reason); }
    }
    pub(super) fn reason(&self) -> &str {
        if self.choice == Some(Choice::Original) { return "explicit original cached control"; }
        if self.choice.is_some() { return "explicit arena cache-disabled control"; }
        if let Some(reason) = &self.failure { return reason; }
        if let Some(reason) = self.diagnostic { return reason; }
        if !self.supported { return "automatic MDI unsupported; original cached rendering"; }
        if self.distance != 128 { return "automatic admission requires exactly render distance 128"; }
        "automatic native MDI eligible at render distance 128"
    }
    pub(super) fn report(&self, active: bool, live: bool) -> serde_json::Value {
        serde_json::json!({
            "automatic": self.automatic(), "override": self.choice.map(|v| format!("{v:?}")),
            "requested_render_distance": self.distance, "automatic_distance": 128,
            "supported": self.supported, "desired_shared": self.desired().is_some(),
            "active": active, "latched_until_empty": active && live && self.desired().is_none(),
            "initialization_failed": self.failure.is_some(), "diagnostic_suppressed": self.diagnostic.is_some(),
            "reason": self.reason(),
            "transition_note": "Live original-to-arena activation preserves dedicated meshes; subsequent ordinary uploads may adopt shared backing, with no migration. Active backing stays latched on distance decrease until all chunks are cleared. Graphics settings are never changed by this policy."
        })
    }
}

#[cfg(test)]
#[path = "renderer_arena_policy_tests.rs"]
mod tests;
