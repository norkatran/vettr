import { type AgentReply, REPLY_KIND_LABEL } from '@shared/replies'
import { createContext, useContext } from 'react'
import type { ResolvedComments } from './useResolvedComments'

/** The agent's answer to a comment. A `resolved` reply is only its belief: the reviewer decides. */
export function ReplyBubble({
  reply,
  where
}: {
  reply: AgentReply
  where?: string
}): React.JSX.Element {
  return (
    <div className={`agent-reply${reply.kind ? ` ${reply.kind}` : ''}`}>
      <div className="comment-meta">
        <span>
          Agent{reply.kind ? ` ${REPLY_KIND_LABEL[reply.kind]}` : ' replied'}
          {where ? ` on ${where}` : ''}
        </span>
      </div>
      <p>{reply.message}</p>
    </div>
  )
}

export const ResolutionContext = createContext<ResolvedComments | null>(null)

/**
 * A sent comment and its replies as one thread the user can resolve or reopen. Resolved threads
 * collapse to their summary line. Resolving is the user's call alone: the agent only says whether
 * it believes the issue is fixed.
 */
export function ResolvableThread({
  id,
  className,
  summary,
  children
}: {
  id: string
  className: string
  summary: string
  children: React.ReactNode
}): React.JSX.Element {
  const resolution = useContext(ResolutionContext)
  if (resolution?.ids.has(id)) {
    return (
      <details className={`${className} resolved-thread`}>
        <summary>Resolved: {summary}</summary>
        {children}
        <div className="comment-actions">
          <button type="button" onClick={() => resolution.set(id, false)}>
            Reopen
          </button>
        </div>
      </details>
    )
  }
  return (
    <div className={className}>
      {children}
      {resolution && (
        <div className="comment-actions">
          <button type="button" onClick={() => resolution.set(id, true)}>
            Resolve
          </button>
        </div>
      )}
    </div>
  )
}
