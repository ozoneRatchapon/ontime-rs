import { Playback, TimerPhase } from 'ontime-types';
import { validatePlayback } from 'ontime-utils';

import { setPlayback } from '../../../common/hooks/useSocket';

export interface PlaybackControlInput {
  playback: Playback;
  numEvents: number;
  selectedEventIndex: number | null;
  timerPhase: TimerPhase;
  /**
   * The connection to the server is lost. Every control is disabled, since a
   * press now would not arrive, and one that arrived late, after the operator
   * gave up on it, would be worse (plan 016).
   */
  isOffline?: boolean;
}

export interface PlaybackControlState {
  // Playback states
  isPlaying: boolean;
  isPaused: boolean;
  isRolling: boolean;
  isArmed: boolean;
  isStopped: boolean;

  // Position states
  isFirst: boolean;
  isLast: boolean;
  noEvents: boolean;

  // Disable flags
  disableGo: boolean;
  disableNext: boolean;
  disablePrev: boolean;
  disableStart: boolean;
  disablePause: boolean;
  disableRoll: boolean;
  disableStop: boolean;
  disableReload: boolean;
  disableAddTime: boolean;

  // Go button configuration
  goAction: () => void;
  goLabel: 'Start' | 'Next' | 'Finish';
}

/**
 * Centralized playback control state calculator.
 * Consolidates all playback logic, disable states, and derived values in one place.
 */
export function getPlaybackControlState({
  playback,
  numEvents,
  selectedEventIndex,
  timerPhase,
  isOffline = false,
}: PlaybackControlInput): PlaybackControlState {
  const isFirst = selectedEventIndex === 0;
  const isLast = selectedEventIndex === numEvents - 1;
  const noEvents = numEvents === 0;
  const isRolling = playback === Playback.Roll;

  const playbackCan = validatePlayback(playback, timerPhase);
  const { action: goAction, label: goLabel } = getGoAction(playback, selectedEventIndex, isLast);

  return {
    isPlaying: playback === Playback.Play,
    isPaused: playback === Playback.Pause,
    isRolling,
    isArmed: playback === Playback.Armed,
    isStopped: playback === Playback.Stop,
    isFirst,
    isLast,
    noEvents,
    disableGo: isOffline || isRolling || noEvents,
    disableNext: isOffline || isRolling || noEvents || isLast,
    disablePrev: isOffline || isRolling || noEvents || isFirst,
    disableStart: isOffline || !playbackCan.start,
    disablePause: isOffline || !playbackCan.pause,
    disableRoll: isOffline || !playbackCan.roll || noEvents,
    disableStop: isOffline || !playbackCan.stop,
    disableReload: isOffline || !playbackCan.reload,
    disableAddTime: isOffline || (playback !== Playback.Play && playback !== Playback.Pause),
    goAction,
    goLabel,
  };
}

/**
 * Determines the action and label for the "Go" button based on playback state.
 */
function getGoAction(
  playback: Playback,
  selectedEventIndex: number | null,
  isLast: boolean,
): { action: () => void; label: 'Start' | 'Next' | 'Finish' } {
  if (playback === Playback.Armed) return { action: setPlayback.start, label: 'Start' };
  if (isLast) return { action: setPlayback.stop, label: 'Finish' };
  if (selectedEventIndex === null) return { action: setPlayback.startNext, label: 'Start' };
  return { action: setPlayback.startNext, label: 'Next' };
}
