// What the stubbed services were asked to do, for gen-integration.ts to read.
// Each case resets it, and sets whether runtime commands succeed.
export type Call = { fn: string; args: unknown[] };

export const record: { calls: Call[]; ok: boolean; rundown: unknown; customFields: unknown } = {
  calls: [],
  ok: true,
  rundown: { id: 'rundown', entries: {} },
  customFields: {},
};

export function recorder(fn: string, result: (...args: unknown[]) => unknown = () => undefined) {
  return (...args: unknown[]) => {
    record.calls.push({ fn, args: JSON.parse(JSON.stringify(args)) });
    return result(...args);
  };
}
