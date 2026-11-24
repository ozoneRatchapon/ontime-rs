/**
 * Stands in for the compiled Rust timer engine in unit tests, which run
 * without a wasm build. Loading fails, so the client takes its fallback path:
 * rendering what the server sends, as it does in a browser without the engine.
 */
export default async function init(): Promise<never> {
  throw new Error('the timer engine is not built for unit tests');
}

export function engine_info(): string {
  return '{}';
}

export class OntimeTimer {
  constructor() {
    throw new Error('the timer engine is not built for unit tests');
  }
  free(): void {}
  clock(): number {
    return 0;
  }
  is_running(): boolean {
    return false;
  }
  skew(): number {
    return 0;
  }
  tick(): string {
    return '';
  }
  update(): boolean {
    return false;
  }
}
