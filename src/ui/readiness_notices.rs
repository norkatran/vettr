//! Announcing readiness transitions (the notification half of `useReadiness.ts`).

use crate::readiness::{describe_transition, NoticeLevel, Readiness};

use super::notifications::{Notifier, NotifyLevel};

/// Tell the user about the change from `before` to `next`, if it deserves a notice. `building`
/// carries the "image build announced" flag between calls.
pub fn notify_transition(
    notifier: &Notifier,
    before: &Readiness,
    next: &Readiness,
    building: &mut bool,
) {
    let result = describe_transition(before, next, *building);
    *building = result.building;
    if let Some(notice) = result.notice {
        let level = match notice.level {
            NoticeLevel::Error => NotifyLevel::Error,
            NoticeLevel::Info => NotifyLevel::Info,
        };
        notifier.notify_level(&notice.title, &notice.detail, level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readiness::{ReadinessReason, ReadinessStatus};

    #[test]
    fn announces_the_image_build_once_and_its_end() {
        let notifier = Notifier::new();
        let mut building = false;
        let idle = Readiness::initial();
        let build =
            Readiness::with_reason(ReadinessStatus::Starting, ReadinessReason::BuildingImage);
        notify_transition(&notifier, &idle, &build, &mut building);
        assert!(building);
        assert_eq!(notifier.len(), 1);
        notify_transition(&notifier, &build, &build, &mut building);
        assert_eq!(notifier.len(), 1);
        let ready = Readiness::new(ReadinessStatus::Ready);
        notify_transition(&notifier, &build, &ready, &mut building);
        assert!(!building);
        assert_eq!(notifier.len(), 2);
    }
}
