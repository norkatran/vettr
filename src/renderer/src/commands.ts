import {
  type Branch,
  deletableBranches,
  type GitAction,
  invalidBranchName,
  mergeTargets,
  switchName,
  switchTargets
} from '@shared/gitActions'
import { sessionLabel } from '@shared/sessions'
import type { PaletteApi, PaletteItem } from './CommandPalette'
import type { View } from './Sidebar'

/** What a command can use. Failures are reported through `notify`, not thrown. */
export interface CommandContext extends PaletteApi {
  /** The open project, or null when none is. */
  project: string | null
  notify: (title: string, detail: string) => void
  /** Number of staged files, to check before asking for a commit message. */
  stagedCount: () => number
  /** Show the Changes view and focus the commit message input. */
  focusCommit: () => void
  /** Switch to a view, expanding the side panel. */
  showView: (view: View) => void
  /** Start a fresh agent session and show the Session view. */
  newSession: () => void
  /** Replace the current session with a stored one and show the Session view. */
  openSession: (id: string) => void
}

/** A global command. Every command is always listed; one that cannot run explains why. */
export interface Command {
  id: string
  category: string
  title: string
  /** Set to false for commands that work without an open project. Default true. */
  needsProject?: boolean
  run: (ctx: CommandContext, project: string) => Promise<void>
}

const failed = (ctx: CommandContext, title: string, failure: string | null): void => {
  if (failure) ctx.notify(`${title} failed`, failure)
}

const act =
  (title: string, action: GitAction): Command['run'] =>
  async (ctx, project) =>
    failed(ctx, title, await window.agentide.runGitAction(project, action))

const branchItem = (b: Branch): PaletteItem => ({
  id: b.ref,
  label: b.ref,
  ...(b.remote && { detail: 'remote' })
})

/** Pick a branch from those `filter` keeps, then run the action built from it. */
const withBranch =
  (
    title: string,
    placeholder: string,
    filter: (branches: Branch[]) => Branch[],
    build: (branch: Branch) => GitAction
  ): Command['run'] =>
  async (ctx, project) => {
    const choices = filter(await window.agentide.listBranches(project))
    if (choices.length === 0) return ctx.notify(`${title} failed`, 'There are no other branches.')
    const ref = await ctx.pick(placeholder, choices.map(branchItem))
    const branch = choices.find((b) => b.ref === ref)
    if (branch) failed(ctx, title, await window.agentide.runGitAction(project, build(branch)))
  }

const publish: Command['run'] = async (ctx, project) => {
  const remotes = await window.agentide.listRemotes(project)
  if (remotes.length === 0) {
    return ctx.notify(
      'Cannot publish branch',
      'This repository has no remotes. Add one with `git remote add`.'
    )
  }
  const remote =
    remotes.length === 1
      ? remotes[0]
      : await ctx.pick(
          'Publish to which remote?',
          remotes.map((r) => ({ id: r, label: r }))
        )
  if (remote) failed(ctx, 'Publish', await window.agentide.publish(project, remote))
}

const newBranch: Command['run'] = async (ctx, project) => {
  const name = (await ctx.input('New branch name'))?.trim()
  if (name === undefined) return
  const invalid = invalidBranchName(name)
  if (invalid) return ctx.notify('Cannot create branch', invalid)
  failed(
    ctx,
    'New branch',
    await window.agentide.runGitAction(project, { kind: 'createBranch', name })
  )
}

const discardAll: Command['run'] = async (ctx, project) => {
  const answer = await ctx.pick(
    'Discard all changes and delete untracked files? This cannot be undone.',
    [
      { id: 'discard', label: 'Discard all changes' },
      { id: 'cancel', label: 'Cancel' }
    ]
  )
  if (answer === 'discard')
    failed(ctx, 'Discard', await window.agentide.runGitAction(project, { kind: 'discardAll' }))
}

const commit: Command['run'] = async (ctx) => {
  if (ctx.stagedCount() === 0) {
    return ctx.notify(
      'Nothing to commit',
      'No files are staged. Stage files first (Git: Stage All).'
    )
  }
  ctx.focusCommit()
}

/** Search the project's stored sessions in a wide palette. Picking one replaces the current session. */
const listSessions: Command['run'] = async (ctx) => {
  const sessions = await window.agentide.listSessions()
  if (sessions.length === 0)
    return ctx.notify('No sessions', 'This project has no stored sessions.')
  const id = await ctx.pick(
    'Search sessions',
    sessions.map((s) => ({ id: s.id, label: sessionLabel(s) })),
    { wide: true }
  )
  if (id) ctx.openSession(id)
}

export const commands: Command[] = [
  {
    id: 'new-session',
    category: 'Session',
    title: 'New Session',
    needsProject: false,
    run: async (ctx) => ctx.newSession()
  },
  { id: 'list-sessions', category: 'Session', title: 'List Sessions', run: listSessions },
  {
    id: 'jump-session',
    category: 'Jump To',
    title: 'Session',
    needsProject: false,
    run: async (ctx) => ctx.showView('session')
  },
  {
    id: 'jump-changes',
    category: 'Jump To',
    title: 'Changes',
    needsProject: false,
    run: async (ctx) => ctx.showView('changes')
  },
  {
    id: 'jump-settings',
    category: 'Jump To',
    title: 'Settings',
    needsProject: false,
    run: async (ctx) => ctx.showView('settings')
  },
  { id: 'fetch', category: 'Git', title: 'Fetch', run: act('Fetch', { kind: 'fetch' }) },
  { id: 'pull', category: 'Git', title: 'Pull', run: act('Pull', { kind: 'pull' }) },
  {
    id: 'push',
    category: 'Git',
    title: 'Push',
    run: async (ctx, project) => failed(ctx, 'Push', await window.agentide.push(project))
  },
  { id: 'publish', category: 'Git', title: 'Publish Branch', run: publish },
  { id: 'new-branch', category: 'Git', title: 'New Branch', run: newBranch },
  {
    id: 'change-branch',
    category: 'Git',
    title: 'Change Branch',
    run: withBranch('Change branch', 'Switch to which branch?', switchTargets, (b) => ({
      kind: 'checkout',
      name: switchName(b)
    }))
  },
  {
    id: 'delete-branch',
    category: 'Git',
    title: 'Delete Branch',
    run: withBranch('Delete branch', 'Delete which branch?', deletableBranches, (b) => ({
      kind: 'deleteBranch',
      name: b.ref
    }))
  },
  { id: 'commit', category: 'Git', title: 'Commit', run: commit },
  {
    id: 'stage-all',
    category: 'Git',
    title: 'Stage All',
    run: act('Stage all', { kind: 'stageAll' })
  },
  {
    id: 'unstage-all',
    category: 'Git',
    title: 'Unstage All',
    run: act('Unstage all', { kind: 'unstageAll' })
  },
  { id: 'discard-all', category: 'Git', title: 'Discard All Changes', run: discardAll },
  { id: 'stash', category: 'Git', title: 'Stash', run: act('Stash', { kind: 'stash' }) },
  {
    id: 'stash-pop',
    category: 'Git',
    title: 'Pop Stash',
    run: act('Pop stash', { kind: 'stashPop' })
  },
  {
    id: 'merge',
    category: 'Git',
    title: 'Merge Branch into Current',
    run: withBranch('Merge', 'Merge which branch into the current one?', mergeTargets, (b) => ({
      kind: 'merge',
      name: b.ref
    }))
  },
  {
    id: 'rebase',
    category: 'Git',
    title: 'Rebase Current Branch onto',
    run: withBranch('Rebase', 'Rebase the current branch onto?', mergeTargets, (b) => ({
      kind: 'rebase',
      name: b.ref
    }))
  }
]

/** Ask which command to run, then run it. Failures become notifications. */
export async function runCommandPalette(ctx: CommandContext): Promise<void> {
  const id = await ctx.pick(
    'Type a command',
    commands.map((c) => ({ id: c.id, label: `${c.category}: ${c.title}` }))
  )
  const command = commands.find((c) => c.id === id)
  if (!command) return
  if (command.needsProject !== false && !ctx.project)
    return ctx.notify('No project open', 'Open a project first (File > Open Project).')
  try {
    await command.run(ctx, ctx.project ?? '')
  } catch (error) {
    ctx.notify(`${command.title} failed`, (error as Error).message)
  }
}
