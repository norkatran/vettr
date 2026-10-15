import { describe, expect, it } from 'vitest'
import { ENVIRONMENT_PROMPT } from './environmentPrompt'

describe('ENVIRONMENT_PROMPT', () => {
  it('tells the agent it has sudo and may install tooling without asking', () => {
    expect(ENVIRONMENT_PROMPT).toContain('passwordless `sudo`')
    expect(ENVIRONMENT_PROMPT).toContain('do not ask permission')
  })

  it('asks the agent to set up the project and install its dependencies', () => {
    expect(ENVIRONMENT_PROMPT).toContain('composer install')
    expect(ENVIRONMENT_PROMPT).toContain('sudo apt-get')
  })

  it('asks the agent to run lint and tests before reporting work done, without weakening them', () => {
    expect(ENVIRONMENT_PROMPT).toMatch(/run the linter and the tests/)
    expect(ENVIRONMENT_PROMPT).toMatch(/Do not disable or weaken tests/)
  })
})
