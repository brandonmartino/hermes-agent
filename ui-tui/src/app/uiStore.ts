import { atom, computed } from 'nanostores'

import { MOUSE_TRACKING } from '../config/env.js'
import { ZERO } from '../domain/usage.js'
import { DEFAULT_THEME } from '../theme.js'

import { DEFAULT_INDICATOR_STYLE, type UiState } from './interfaces.js'

const buildUiState = (): UiState => ({
  bgTasks: new Set(),
  busy: false,
  busyInputMode: 'queue',
  compact: false,
  detailsMode: 'collapsed',
  detailsModeCommandOverride: false,
  indicatorStyle: DEFAULT_INDICATOR_STYLE,
  info: null,
  liveSessionCount: 0,
  inlineDiffs: true,
  mouseTracking: MOUSE_TRACKING,
  pasteCollapseLines: 5,
  pasteCollapseChars: 2000,
  sections: {},
  showCost: false,
  showReasoning: false,
  sid: null,
  status: 'summoning hermes…',
  statusBar: 'top',
  streaming: true,
  theme: DEFAULT_THEME,
  usage: ZERO
})

export const $uiState = atom<UiState>(buildUiState())

export const $uiTheme = computed($uiState, state => state.theme)
export const $uiSessionId = computed($uiState, state => state.sid)

export const getUiState = () => $uiState.get()

/**
 * The status to fall back to once a transient/prompt-specific status
 * ('waiting for input…', 'sudo password needed', 'secret input needed') no
 * longer applies — e.g. when a dead clarify/sudo/secret overlay is dismissed
 * via the null-RPC fallback, or when a prompt.expire clears the overlay. If
 * the agent is mid-turn it's 'running…'; otherwise 'ready'. Single-sourced so
 * the gateway-event handler and the useMainApp fallbacks can't drift.
 */
export const statusFromBusy = (): string => (getUiState().busy ? 'running…' : 'ready')

export const patchUiState = (next: Partial<UiState> | ((state: UiState) => UiState)) =>
  $uiState.set(typeof next === 'function' ? next($uiState.get()) : { ...$uiState.get(), ...next })

export const resetUiState = () => $uiState.set(buildUiState())
