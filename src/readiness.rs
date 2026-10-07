//! Lifecycle of the sandbox agent, owned by the backend and pushed to the UI (port of `src/shared/readiness.ts`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadinessStatus {
    Idle,
    Starting,
    Ready,
    Stopping,
    Error,
}

/// Why the agent is not ready (absent when it is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadinessReason {
    NoKey,
    DockerUnavailable,
    BuildingImage,
    Starting,
    CrashedRestarting,
    Offline,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub status: ReadinessStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<ReadinessReason>,
    /// A user-facing explanation, for example Docker's own message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Readiness {
    /// A readiness with just a status.
    pub fn new(status: ReadinessStatus) -> Readiness {
        Readiness {
            status,
            reason: None,
            message: None,
        }
    }

    /// A readiness with a status and a reason.
    pub fn with_reason(status: ReadinessStatus, reason: ReadinessReason) -> Readiness {
        Readiness {
            status,
            reason: Some(reason),
            message: None,
        }
    }

    /// The state before anything has happened (`INITIAL_READINESS` in the TS).
    pub fn initial() -> Readiness {
        Readiness::new(ReadinessStatus::Idle)
    }
}

impl Default for Readiness {
    fn default() -> Readiness {
        Readiness::initial()
    }
}

/// Whether prompts, review comments and any other input directed at the agent are allowed.
pub fn can_direct_agent(readiness: &Readiness) -> bool {
    readiness.status == ReadinessStatus::Ready
}

fn reason_text(reason: ReadinessReason) -> &'static str {
    match reason {
        ReadinessReason::NoKey => "Add an API key or token to start the agent.",
        ReadinessReason::DockerUnavailable => "Docker is not available.",
        ReadinessReason::BuildingImage => "Building the sandbox image\u{2026}",
        ReadinessReason::Starting => "Starting the agent\u{2026}",
        ReadinessReason::CrashedRestarting => {
            "The agent stopped unexpectedly and is restarting\u{2026}"
        }
        ReadinessReason::Offline => "You appear to be offline.",
        ReadinessReason::Error => "The agent could not be started.",
    }
}

/// Short text saying why agent inputs are disabled, or `None` when they are enabled.
pub fn readiness_block_reason(readiness: &Readiness) -> Option<String> {
    if can_direct_agent(readiness) {
        return None;
    }
    if let Some(message) = &readiness.message {
        if !message.is_empty() {
            return Some(message.clone());
        }
    }
    if let Some(reason) = readiness.reason {
        return Some(reason_text(reason).to_string());
    }
    if readiness.status == ReadinessStatus::Stopping {
        Some("Stopping the agent\u{2026}".to_string())
    } else {
        Some("The agent is not running.".to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    Error,
    Info,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessNotice {
    pub title: String,
    pub detail: String,
    pub level: NoticeLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionResult {
    pub notice: Option<ReadinessNotice>,
    /// Whether a sandbox image build has been announced and has not finished (carry to the next call).
    pub building: bool,
}

fn notice(title: &str, detail: &str, level: NoticeLevel) -> Option<ReadinessNotice> {
    Some(ReadinessNotice {
        title: title.to_string(),
        detail: detail.to_string(),
        level,
    })
}

/// What to tell the user when readiness goes from `before` to `next`, if anything. `building` is
/// the flag returned by the previous call: the image build ends in `starting` and then `ready`,
/// so "built" is announced when `ready` arrives while it is still set.
pub fn describe_transition(
    before: &Readiness,
    next: &Readiness,
    building: bool,
) -> TransitionResult {
    let is_building = if next.reason == Some(ReadinessReason::BuildingImage) {
        true
    } else {
        next.status == ReadinessStatus::Starting && building
    };
    let keep = |n: Option<ReadinessNotice>| -> TransitionResult {
        TransitionResult {
            notice: n,
            building: is_building,
        }
    };
    if next.reason == Some(ReadinessReason::BuildingImage) {
        return keep(if before.reason == Some(ReadinessReason::BuildingImage) {
            None
        } else {
            notice(
                "Building the sandbox image",
                "This happens once and can take a few minutes. The agent starts when it is done.",
                NoticeLevel::Info,
            )
        });
    }
    if next.status == ReadinessStatus::Ready {
        if building {
            return keep(notice(
                "Sandbox image built",
                "The agent is ready.",
                NoticeLevel::Info,
            ));
        }
        return keep(
            if before.reason == Some(ReadinessReason::CrashedRestarting) {
                notice(
                    "Agent restarted",
                    "The session can be resumed by sending a message.",
                    NoticeLevel::Info,
                )
            } else {
                None
            },
        );
    }
    if next.reason == Some(ReadinessReason::DockerUnavailable) {
        return keep(
            if before.reason == Some(ReadinessReason::DockerUnavailable) {
                None
            } else {
                let detail = match &next.message {
                    Some(m) => m.clone(),
                    None => "The agent cannot start.".to_string(),
                };
                notice("Docker is not available", &detail, NoticeLevel::Error)
            },
        );
    }
    if next.status == ReadinessStatus::Error {
        let detail = match &next.message {
            Some(m) => m.clone(),
            None => "Unknown error.".to_string(),
        };
        return keep(notice(
            "The agent could not be started",
            &detail,
            NoticeLevel::Error,
        ));
    }
    if next.reason == Some(ReadinessReason::CrashedRestarting)
        && before.reason != Some(ReadinessReason::CrashedRestarting)
    {
        return keep(notice(
            "Agent stopped",
            "The agent stopped unexpectedly. Restarting it\u{2026}",
            NoticeLevel::Info,
        ));
    }
    keep(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Readiness {
        Readiness::new(ReadinessStatus::Ready)
    }

    #[test]
    fn can_direct_agent_only_when_ready() {
        assert!(can_direct_agent(&ready()));
        for status in [
            ReadinessStatus::Idle,
            ReadinessStatus::Starting,
            ReadinessStatus::Stopping,
            ReadinessStatus::Error,
        ] {
            assert!(!can_direct_agent(&Readiness::new(status)));
        }
        assert!(!can_direct_agent(&Readiness::initial()));
    }

    #[test]
    fn block_reason_is_none_when_ready() {
        assert_eq!(readiness_block_reason(&ready()), None);
    }

    #[test]
    fn block_reason_prefers_the_explicit_message() {
        let r = Readiness {
            status: ReadinessStatus::Error,
            reason: Some(ReadinessReason::Error),
            message: Some("boom".to_string()),
        };
        assert_eq!(readiness_block_reason(&r), Some("boom".to_string()));
    }

    #[test]
    fn block_reason_has_text_for_every_reason() {
        let reasons = [
            ReadinessReason::NoKey,
            ReadinessReason::DockerUnavailable,
            ReadinessReason::BuildingImage,
            ReadinessReason::Starting,
            ReadinessReason::CrashedRestarting,
            ReadinessReason::Offline,
            ReadinessReason::Error,
        ];
        for reason in reasons {
            let text =
                readiness_block_reason(&Readiness::with_reason(ReadinessStatus::Error, reason))
                    .unwrap();
            assert!(text.chars().any(|c| !c.is_whitespace()));
        }
    }

    #[test]
    fn block_reason_falls_back_on_the_status() {
        assert_eq!(
            readiness_block_reason(&Readiness::new(ReadinessStatus::Stopping)),
            Some("Stopping the agent\u{2026}".to_string())
        );
        assert_eq!(
            readiness_block_reason(&Readiness::new(ReadinessStatus::Idle)),
            Some("The agent is not running.".to_string())
        );
    }

    fn building_state() -> Readiness {
        Readiness::with_reason(ReadinessStatus::Starting, ReadinessReason::BuildingImage)
    }

    fn starting_state() -> Readiness {
        Readiness::with_reason(ReadinessStatus::Starting, ReadinessReason::Starting)
    }

    fn error_state(reason: Option<ReadinessReason>, message: Option<&str>) -> Readiness {
        Readiness {
            status: ReadinessStatus::Error,
            reason,
            message: message.map(|m| m.to_string()),
        }
    }

    #[test]
    fn announces_an_image_build_once_and_the_end_of_it_when_ready() {
        let building = building_state();
        let starting = starting_state();
        let first = describe_transition(&Readiness::initial(), &building, false);
        assert_eq!(
            first.notice.as_ref().map(|n| n.title.as_str()),
            Some("Building the sandbox image")
        );
        assert!(first.building);
        let mut progress_state = building.clone();
        progress_state.message = Some("step 2".to_string());
        let progress = describe_transition(&building, &progress_state, true);
        assert_eq!(
            progress,
            TransitionResult {
                notice: None,
                building: true
            }
        );
        let warming = describe_transition(&building, &starting, true);
        assert_eq!(
            warming,
            TransitionResult {
                notice: None,
                building: true
            }
        );
        let done = describe_transition(&starting, &ready(), true);
        assert_eq!(
            done.notice.as_ref().map(|n| n.title.as_str()),
            Some("Sandbox image built")
        );
        assert!(!done.building);
    }

    #[test]
    fn stops_tracking_a_build_that_failed() {
        let failed = describe_transition(
            &building_state(),
            &error_state(Some(ReadinessReason::Error), Some("x")),
            true,
        );
        assert_eq!(
            failed.notice.as_ref().map(|n| n.level),
            Some(NoticeLevel::Error)
        );
        assert!(!failed.building);
    }

    #[test]
    fn reports_docker_being_unavailable_once() {
        let next = error_state(Some(ReadinessReason::DockerUnavailable), Some("no docker"));
        assert_eq!(
            describe_transition(&Readiness::initial(), &next, false).notice,
            Some(ReadinessNotice {
                title: "Docker is not available".to_string(),
                detail: "no docker".to_string(),
                level: NoticeLevel::Error,
            })
        );
        assert_eq!(describe_transition(&next, &next, false).notice, None);
        let no_message = error_state(Some(ReadinessReason::DockerUnavailable), None);
        assert_eq!(
            describe_transition(&Readiness::initial(), &no_message, false)
                .notice
                .map(|n| n.detail),
            Some("The agent cannot start.".to_string())
        );
    }

    #[test]
    fn reports_other_errors_with_their_message_or_a_fallback() {
        let with_message = describe_transition(
            &starting_state(),
            &error_state(Some(ReadinessReason::Error), Some("boom")),
            false,
        );
        assert_eq!(
            with_message.notice.map(|n| n.detail),
            Some("boom".to_string())
        );
        let fallback = describe_transition(&starting_state(), &error_state(None, None), false);
        assert_eq!(
            fallback.notice.map(|n| n.detail),
            Some("Unknown error.".to_string())
        );
    }

    #[test]
    fn reports_a_crash_restart_and_its_recovery() {
        let crashed = Readiness::with_reason(
            ReadinessStatus::Starting,
            ReadinessReason::CrashedRestarting,
        );
        assert_eq!(
            describe_transition(&ready(), &crashed, false)
                .notice
                .map(|n| n.title),
            Some("Agent stopped".to_string())
        );
        assert_eq!(describe_transition(&crashed, &crashed, false).notice, None);
        assert_eq!(
            describe_transition(&crashed, &ready(), false)
                .notice
                .map(|n| n.title),
            Some("Agent restarted".to_string())
        );
    }

    #[test]
    fn says_nothing_for_ordinary_transitions() {
        assert_eq!(
            describe_transition(&Readiness::initial(), &starting_state(), false).notice,
            None
        );
        assert_eq!(
            describe_transition(&starting_state(), &ready(), false).notice,
            None
        );
    }

    #[test]
    fn serialises_to_the_ts_strings() {
        let r = Readiness::with_reason(ReadinessStatus::Starting, ReadinessReason::BuildingImage);
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            "{\"status\":\"starting\",\"reason\":\"building-image\"}"
        );
    }
}
