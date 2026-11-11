//! Local-only engagement counter. Never checks GitHub, sends telemetry, or nags.
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SupportPrompt {
    sessions: u32,
    active_seconds: f64,
    shown: bool,
    #[serde(skip)]
    active: bool,
}
impl SupportPrompt {
    /// Returns true when a completed handoff should be persisted.
    pub fn update(&mut self, active: bool, elapsed: f64) -> bool {
        if active && self.active {
            // A suspended process must not count sleeping time as audio use.
            self.active_seconds += elapsed.clamp(0., 2.);
        }
        let completed = self.active && !active;
        if completed {
            self.sessions = self.sessions.saturating_add(1);
        }
        self.active = active;
        completed
    }
    pub fn due(&self, settings_visible: bool, app_active: bool) -> bool {
        !self.shown
            && !self.active
            && self.sessions >= 3
            && self.active_seconds >= 600.
            && settings_visible
            && app_active
    }
    pub fn mark_shown(&mut self) {
        self.shown = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(prompt: &mut SupportPrompt) {
        prompt.update(true, 0.);
        for _ in 0..200 {
            assert!(!prompt.update(true, 1.));
        }
        assert!(prompt.update(false, 1.));
    }
    #[test]
    fn prompts_once_after_use_when_idle_and_settings_are_active() {
        let mut prompt = SupportPrompt::default();
        assert!(!prompt.due(true, true));
        session(&mut prompt);
        session(&mut prompt);
        assert!(!prompt.due(true, true));
        session(&mut prompt);
        assert!(!prompt.due(false, true));
        assert!(!prompt.due(true, false));
        assert!(prompt.due(true, true));
        prompt.update(true, 0.);
        assert!(!prompt.due(true, true));
        prompt.update(false, 0.);
        prompt.mark_shown();
        let saved = serde_json::to_string(&prompt).unwrap();
        let mut restored: SupportPrompt = serde_json::from_str(&saved).unwrap();
        session(&mut restored);
        assert!(!restored.due(true, true));
    }
    #[test]
    fn heartbeat_and_sleep_do_not_inflate_completed_handoffs_or_usage() {
        let mut prompt = SupportPrompt::default();
        for _ in 0..1000 {
            prompt.update(false, 1.);
        }
        prompt.update(true, 0.);
        prompt.update(true, 36000.);
        assert_eq!(prompt.active_seconds, 2.);
        assert_eq!(prompt.sessions, 0);
        prompt.update(false, 0.);
        assert_eq!(prompt.sessions, 1);
        let restored: SupportPrompt =
            serde_json::from_str(&serde_json::to_string(&prompt).unwrap()).unwrap();
        assert!(!restored.active);
        assert!(!restored.due(true, true));
    }
}
