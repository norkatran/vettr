import type { SlashCommandInfo } from '@shared/agent'
import { useEffect, useState } from 'react'

/**
 * The slash commands the agent offers. The list is replaced whenever the agent reports one
 * (at warm-up and when skills change mid-session) and emptied when the agent exits.
 */
export function useSlashCommands(): SlashCommandInfo[] {
  const [commands, setCommands] = useState<SlashCommandInfo[]>([])

  useEffect(() => {
    let live = true
    let received = false
    const unsubscribe = window.vettr.onAgentEvent((event) => {
      if (event.type === 'commands') {
        received = true
        setCommands(event.commands)
      } else if (event.type === 'exited') {
        setCommands([])
      }
    })
    // The agent may have reported before this view loaded
    void window.vettr.getSlashCommands().then((current) => {
      if (live && !received) setCommands(current)
    })
    return () => {
      live = false
      unsubscribe()
    }
  }, [])

  return commands
}
