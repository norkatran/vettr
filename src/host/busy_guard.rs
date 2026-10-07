//! Confirmation before an action that would stop the agent mid-turn (port of `src/main/busyGuard.ts`).

pub const CANCELLED_WHILE_BUSY: &str = "Cancelled. The agent is still working.";

/// Ask before an action that would stop the agent mid-turn and lose its work in progress. Does
/// nothing when the agent is idle, and returns an error when the user declines.
pub fn confirm_if_busy(is_busy: bool, ask: impl FnOnce() -> bool) -> Result<(), String> {
    if is_busy && !ask() {
        return Err(CANCELLED_WHILE_BUSY.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn does_not_ask_when_the_agent_is_idle() {
        let asked = Cell::new(false);
        let result = confirm_if_busy(false, || {
            asked.set(true);
            true
        });
        assert!(result.is_ok());
        assert!(!asked.get());
    }

    #[test]
    fn lets_the_action_proceed_when_the_user_confirms() {
        assert_eq!(confirm_if_busy(true, || true), Ok(()));
    }

    #[test]
    fn errors_when_the_user_declines() {
        assert_eq!(
            confirm_if_busy(true, || false),
            Err(CANCELLED_WHILE_BUSY.to_string())
        );
    }
}
