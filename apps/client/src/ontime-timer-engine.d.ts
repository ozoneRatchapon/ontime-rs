/**
 * Types for the Rust timer engine (packages/ontime-wasm), which vite resolves
 * through an alias to its gitignored build output. Declared here so the client
 * typechecks without building the wasm first, as in CI.
 * Keep in step with packages/ontime-wasm/src: the generated pkg/ontime_timer.d.ts
 * is the reference.
 */
declare module 'ontime-timer-engine' {
  export class OntimeTimer {
    constructor();
    free(): void;
    /** Milliseconds since local midnight, for the wall clock display */
    clock(client_now: number): number;
    /** Whether the timer is advancing */
    is_running(): boolean;
    /** How far the browser clock is from the server's, in milliseconds */
    skew(): number;
    /** The TimerState for this frame as JSON; empty before the first packet */
    tick(client_now: number): string;
    /** Feeds the _engine block of a runtime-data packet; false if it could not be read */
    update(engine_json: string, client_now: number): boolean;
  }
  /** Version marker of the engine build */
  export function engine_info(): string;
  export default function init(
    input?: RequestInfo | URL | Response | BufferSource | WebAssembly.Module,
  ): Promise<unknown>;
}
