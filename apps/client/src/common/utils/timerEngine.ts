/**
 * Local timer engine.
 *
 * The Rust backend sends playback state only when it *changes* and otherwise stays
 * silent, so this module advances the timer between packets. It runs the very same
 * `ontime-core` code the server runs, compiled to WebAssembly, which is why the two
 * cannot drift apart the way a second implementation in TypeScript would.
 *
 * Everything here degrades to a no-op if the module fails to load: the store then
 * simply shows whatever the server last sent, which the server's heartbeat keeps
 * roughly current.
 */

import init, { engine_info as engineInfo, OntimeTimer } from 'ontime-timer-engine';

import { patchRuntimeProperty } from '../stores/runtime';

/**
 * How often the store is patched while something is running. The countdown is read
 * to the second and progress bars interpolate, so there is nothing to gain from
 * patching on every frame — and every patch notifies each subscriber.
 */
const PATCH_INTERVAL_MS = 50;

/**
 * How often the wall clock is patched while nothing is running.
 *
 * The clock has to keep moving even when playback is stopped, or the operator
 * presses start against a display that is up to a second stale and the countdown
 * appears to begin out of step. Doing it here rather than with a server heartbeat
 * keeps an idle room costing nothing.
 */
const IDLE_PATCH_INTERVAL_MS = 200;

let engine: OntimeTimer | null = null;
let frame: number | null = null;
let lastPatch = 0;

/**
 * Diagnostics. The failure modes here are all silent by design — the timer keeps
 * working off the server's heartbeat — which makes "is the engine actually driving
 * this?" impossible to answer by looking at the UI. These log once, never per packet.
 */
const announced = { active: false, noPayload: false, rejected: false };

/** Readable from the console: `window.__ontimeTimerEngine.status()` */
export type EngineStatus = {
  loaded: boolean;
  driving: boolean;
  /** Browser clock minus server clock, in milliseconds. */
  skewMs: number | null;
};

export function getEngineStatus(): EngineStatus {
  return {
    loaded: engine !== null,
    driving: announced.active,
    skewMs: engine ? engine.skew() : null,
  };
}

/**
 * Loads the WebAssembly engine. Safe to call more than once.
 * Resolves to false when the engine is unavailable.
 */
export async function initTimerEngine(): Promise<boolean> {
  if (engine) {
    return true;
  }
  try {
    await init();
    engine = new OntimeTimer();
    // a handle for checking from the console whether the engine is really driving
    // the timer, which the UI alone cannot tell you
    (window as unknown as Record<string, unknown>).__ontimeTimerEngine = {
      status: getEngineStatus,
      info: () => JSON.parse(engineInfo()),
    };
    return true;
  } catch (error) {
    // not fatal: the server heartbeat still drives the timer, just less smoothly
    console.warn('[ontime] local timer engine unavailable, falling back to server updates', error);
    return false;
  }
}

/**
 * Feeds the `_engine` block of a runtime-data packet and (re)starts the local loop.
 * Returns false when the block was missing or unreadable, so the caller knows the
 * server's own values are the only source.
 */
export function applyEngineState(enginePayload: unknown): boolean {
  if (!engine) {
    return false;
  }

  if (!enginePayload) {
    if (!announced.noPayload) {
      announced.noPayload = true;
      console.warn(
        '[ontime] runtime-data has no _engine block — the backend is not the Rust worker, ' +
          'or predates it. Falling back to server updates.',
      );
    }
    return false;
  }

  const accepted = engine.update(JSON.stringify(enginePayload), Date.now());
  if (!accepted) {
    if (!announced.rejected) {
      announced.rejected = true;
      console.warn('[ontime] the _engine block could not be read; falling back to server updates', enginePayload);
    }
    return false;
  }

  if (!announced.active) {
    announced.active = true;
    console.info(`[ontime] local timer engine active (clock skew ${Math.round(engine.skew())}ms)`);
  }

  // patch once immediately so a state change lands without waiting for a frame
  lastPatch = 0;
  schedule();
  return true;
}

/** Stops the loop; call when the socket closes so a stale timer does not keep running. */
export function stopTimerEngine() {
  if (frame !== null) {
    cancelAnimationFrame(frame);
    frame = null;
  }
}

/**
 * Schedules the next frame, replacing any already pending.
 *
 * Every packet restarts the loop, so without cancelling first each one would
 * leave its predecessor running and the loops would accumulate for the length of
 * the event.
 */
function schedule() {
  if (frame !== null) {
    cancelAnimationFrame(frame);
  }
  frame = requestAnimationFrame(tick);
}

function tick() {
  frame = null;

  if (!engine) {
    return;
  }

  const running = engine.is_running();
  const now = Date.now();
  const interval = running ? PATCH_INTERVAL_MS : IDLE_PATCH_INTERVAL_MS;

  if (now - lastPatch >= interval) {
    lastPatch = now;

    patchRuntimeProperty('clock', engine.clock(now));

    // there is no timer to advance while nothing is running; the server's last
    // packet already holds the correct idle state
    if (running) {
      const timer = engine.tick(now);
      if (timer) {
        patchRuntimeProperty('timer', JSON.parse(timer));
      }
    }
  }

  // requestAnimationFrame pauses on a hidden tab, which is what we want: the next
  // frame after it wakes recomputes from the absolute start time, so nothing drifts.
  // The loop continues while idle so the clock does not freeze.
  schedule();
}
