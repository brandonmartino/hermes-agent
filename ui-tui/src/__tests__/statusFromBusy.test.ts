import { afterEach, describe, expect, it } from 'vitest'

import { patchUiState, resetUiState, statusFromBusy } from '../app/uiStore.js'

describe('statusFromBusy', () => {
  afterEach(() => {
    resetUiState()
  })

  it("returns 'running…' while the agent is mid-turn", () => {
    patchUiState({ busy: true })
    expect(statusFromBusy()).toBe('running…')
  })

  it("returns 'ready' when the agent is idle", () => {
    patchUiState({ busy: false })
    expect(statusFromBusy()).toBe('ready')
  })

  it('reflects the live busy flag at call time, not at import time', () => {
    patchUiState({ busy: false })
    expect(statusFromBusy()).toBe('ready')
    patchUiState({ busy: true })
    expect(statusFromBusy()).toBe('running…')
    patchUiState({ busy: false })
    expect(statusFromBusy()).toBe('ready')
  })

  it('never leaves the bar on a transient prompt status after a dead-overlay dismissal', () => {
    // Simulate the null-RPC fallback path: a clarify/sudo/secret request set a
    // prompt-specific status, the prompt died, and the overlay was dismissed.
    // The fallback resets via statusFromBusy(); assert it can never resolve to
    // one of the transient prompt strings.
    const transient = ['waiting for input…', 'sudo password needed', 'secret input needed']

    for (const busy of [true, false]) {
      patchUiState({ busy, status: 'sudo password needed' })
      const next = statusFromBusy()
      expect(transient).not.toContain(next)
      expect(next).toBe(busy ? 'running…' : 'ready')
    }
  })
})
