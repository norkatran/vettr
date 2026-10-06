export const CANCELLED_WHILE_BUSY = 'Cancelled. The agent is still working.'

/**
 * Ask before an action that would stop the agent mid-turn and lose its work in progress. Does
 * nothing when the agent is idle, and throws (so IPC callers report it) when the user declines.
 */
export async function confirmIfBusy(isBusy: boolean, ask: () => Promise<boolean>): Promise<void> {
  if (isBusy && !(await ask())) throw new Error(CANCELLED_WHILE_BUSY)
}
