export interface ReviewRoundSteps {
  /** Record the working tree as the round's baseline, before the agent touches it. */
  snapshot(): Promise<string | null>
  /** Hand the review to the agent; resolves to null on success or to an error message. */
  deliver(): Promise<string | null>
  /** Mark the comments as sent and start the next round (only called after delivery). */
  markSent(baseline: string | null): void
}

/**
 * Send a review round. The comments are marked as sent only once the agent has taken the message,
 * so a failed delivery leaves them pending to be sent again. Resolves to null or the error.
 */
export async function sendReviewRound(steps: ReviewRoundSteps): Promise<string | null> {
  const baseline = await steps.snapshot()
  const error = await steps.deliver()
  if (!error) steps.markSent(baseline)
  return error
}
