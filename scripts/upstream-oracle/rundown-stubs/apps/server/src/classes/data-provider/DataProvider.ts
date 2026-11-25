// The project's storage, in memory: gen-rundown_edits.ts drives the rundown
// service, and what it writes down is not part of the contract.
const rundowns: Record<string, unknown> = {};
let customFields: Record<string, unknown> = {};
let teleprompter: unknown = {};

export const getDataProvider = () => ({
  getProjectRundowns: () => rundowns,
  setRundown: async (id: string, rundown: unknown) => {
    rundowns[id] = rundown;
  },
  getCustomFields: () => customFields,
  setCustomFields: async (fields: Record<string, unknown>) => {
    customFields = fields;
  },
  getTeleprompterSettings: () => teleprompter,
  setTeleprompterSettings: async (settings: unknown) => {
    teleprompter = settings;
  },
});
