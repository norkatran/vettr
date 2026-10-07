//! Sending a review round (port of `src/shared/reviewSend.ts`). The steps are blocking; the
//! backend runs the round on a worker thread.

pub trait ReviewRoundSteps {
    /// Record the working tree as the round's baseline, before the agent touches it.
    fn snapshot(&mut self) -> Option<String>;
    /// Hand the review to the agent; `Err` carries the message to show the user.
    fn deliver(&mut self) -> Result<(), String>;
    /// Mark the comments as sent and start the next round (only called after delivery).
    fn mark_sent(&mut self, baseline: Option<String>);
}

/// Send a review round. The comments are marked as sent only once the agent has taken the message,
/// so a failed delivery leaves them pending to be sent again.
pub fn send_review_round(steps: &mut dyn ReviewRoundSteps) -> Result<(), String> {
    let baseline = steps.snapshot();
    steps.deliver()?;
    steps.mark_sent(baseline);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        order: Vec<String>,
        deliver_result: Result<(), String>,
        marked: usize,
    }

    impl ReviewRoundSteps for Fake {
        fn snapshot(&mut self) -> Option<String> {
            self.order.push("snapshot".to_string());
            Some("tree1".to_string())
        }
        fn deliver(&mut self) -> Result<(), String> {
            self.order.push("deliver".to_string());
            self.deliver_result.clone()
        }
        fn mark_sent(&mut self, baseline: Option<String>) {
            self.marked += 1;
            self.order
                .push(format!("markSent:{}", baseline.unwrap_or_default()));
        }
    }

    fn fake(deliver_result: Result<(), String>) -> Fake {
        Fake {
            order: Vec::new(),
            deliver_result,
            marked: 0,
        }
    }

    #[test]
    fn snapshots_delivers_then_marks_the_comments_sent_with_the_baseline() {
        let mut f = fake(Ok(()));
        assert_eq!(send_review_round(&mut f), Ok(()));
        assert_eq!(f.order, vec!["snapshot", "deliver", "markSent:tree1"]);
    }

    #[test]
    fn leaves_the_comments_pending_when_delivery_fails() {
        let mut f = fake(Err("The agent is not running.".to_string()));
        assert_eq!(
            send_review_round(&mut f),
            Err("The agent is not running.".to_string())
        );
        assert_eq!(f.marked, 0);
    }
}
