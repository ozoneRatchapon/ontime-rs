// The project file contract (plan 020): what upstream's own parseDatabaseModel
// makes of a project file, which is what the Ontime app does with a file it
// opens or imports. tests/project_file_tests.rs imports the same file into a
// room (`POST /data/db/import`) and exports it again (`POST /data/db/export`);
// the export must be what upstream parsed, so the app opens it unchanged.
//
// `data` is null when upstream refuses the file. A project id or rundown id
// upstream had to make up is recorded as "<made>", since it is random.
import {
  makeOntimeDelay,
  makeOntimeGroup,
  makeOntimeMilestone,
} from '@upstream/apps/server/src/api-data/rundown/__mocks__/rundown.mocks.ts';
import { parseDatabaseModel } from '@upstream/apps/server/src/api-data/db/db.parser.ts';
import { demoDb } from '@upstream/apps/server/src/models/demoProject.ts';

import { ev, H, MIN, rundownOf } from './rundown.ts';

const settings = { version: '4.2.0', timeFormat: '12', language: 'de' };

const cases: [string, Record<string, unknown>][] = [
  ['the demo project', structuredClone(demoDb) as any],
  ['settings alone', { settings }],
  ['no settings is not a project file', { project: { title: 'x' } }],
  ['settings without a version is not a project file', { settings: { timeFormat: '12' } }],
  [
    'every section given',
    {
      projectId: 'kept-id',
      settings: { ...settings, editorKey: '1234', operatorKey: null, auxTimerNames: ['A', 'B', 'C'] },
      project: {
        title: 'Gala',
        description: 'd',
        url: 'https://example.com',
        info: 'wifi',
        logo: null,
        custom: [{ title: 'T', value: 'V' }],
      },
      viewSettings: { overrideStyles: true, normalColor: '#fff', warningColor: '#ff0', dangerColor: '#f00' },
      urlPresets: [
        { enabled: true, alias: 'stage', target: 'timer', search: 'a=1', displayInNav: true },
        { alias: 'bare', target: 'clock' },
      ],
      customFields: {
        Notes: { type: 'text', colour: '#123456', label: 'Notes' },
        Image: { type: 'image', colour: '', label: 'Image' },
      },
      automation: { enabledAutomations: false, enabledOscIn: true, oscPortIn: 9000, triggers: [], automations: {} },
      teleprompter: { script: 'note', charsPerLine: 40, heading: 'cue' },
      rundowns: {
        show: rundownOf([ev('a', 10 * H, 30 * MIN, { custom: { Notes: 'hi', Gone: 'x' } }), ev('b', 11 * H, 30 * MIN)]),
      },
    },
  ],
  [
    'invalid parts are dropped or defaulted',
    {
      settings: { version: '4.0.0', auxTimerNames: ['only one'] },
      project: { title: 'P', custom: [{ title: 'ok', value: 'v', url: 'u' }, { title: 1 }, 'text'] },
      viewSettings: { normalColor: '#abc' },
      urlPresets: [{ alias: '', target: 'timer' }, { alias: 'x' }, { alias: 'ok', target: 'cuesheet', options: { a: 1 } }],
      customFields: {
        good: { type: 'text', colour: '#000', label: 'good' },
        wrongKey: { type: 'text', colour: '#000', label: 'Other' },
        number: { type: 'number', colour: '#000', label: 'number' },
        empty: { type: 'text', colour: '#000', label: '' },
        noColour: { type: 'text', label: 'noColour' },
      },
      automation: { oscPortIn: 7000, triggers: 'not a list', automations: null },
      teleprompter: { charsPerLine: 500, heading: 'loud', script: 3 },
      rundowns: {
        show: rundownOf([ev('a', 10 * H, 30 * MIN, { custom: { good: 'g', wrongKey: 'w' } })]),
      },
    },
  ],
  [
    'a project custom list that is not a list',
    { settings, project: { title: 'P', custom: 'nope' } },
  ],
  [
    'a rundown with groups, delays, milestones and broken references',
    {
      settings,
      rundowns: {
        show: {
          ...rundownOf(
            [
              ev('a', 9 * H, 30 * MIN),
              makeOntimeDelay({ id: 'd', duration: 5 * MIN }),
              makeOntimeGroup({ id: 'g', title: 'Block', entries: ['x', 'missing', 'm'] }),
              makeOntimeMilestone({ id: 'top', title: 'Doors' }),
              ev('c', 11 * H, 30 * MIN),
            ],
            [
              ev('x', 10 * H, 30 * MIN, { parent: 'g' }),
              makeOntimeMilestone({ id: 'm', title: 'Inside', parent: 'g' }),
            ],
          ),
          order: ['a', 'd', 'g', 'top', 'nowhere', 'c'],
        },
      },
    },
  ],
  [
    'a legacy block is read as a group',
    {
      settings,
      rundowns: {
        show: rundownOf(
          [{ id: 'b', type: 'block', title: 'Old block', entries: ['x'], colour: '', custom: {} } as any],
          [ev('x', 10 * H, 30 * MIN, { parent: 'b' })],
        ),
      },
    },
  ],
  [
    'an entry of unknown type is skipped',
    {
      settings,
      rundowns: {
        show: rundownOf([ev('a', 10 * H, 30 * MIN), { id: 'q', type: 'quiz' } as any]),
      },
    },
  ],
  [
    'several rundowns, the first one in the file is loaded',
    {
      settings,
      rundowns: {
        zebra: { ...rundownOf([ev('z', 10 * H, 30 * MIN)]), id: 'zebra', title: 'Z' },
        alpha: { ...rundownOf([ev('y', 10 * H, 30 * MIN)]), id: 'alpha', title: 'A' },
      },
    },
  ],
  ['an empty rundowns object', { settings, rundowns: {} }],
];

const made = (value: unknown, given: unknown) => (value === given ? value : '<made>');

const out = cases.map(([name, input]) => {
  let data: any = null;
  try {
    data = parseDatabaseModel(structuredClone(input) as any).data;
  } catch {
    data = null;
  }
  if (data) {
    data.projectId = made(data.projectId, input.projectId);
    // a rundown upstream makes for a file without one has a random id
    const given = (input.rundowns ?? {}) as Record<string, unknown>;
    if (Object.keys(given).length === 0) {
      const [rundown] = Object.values(data.rundowns) as any[];
      data.rundowns = { '<made>': { ...rundown, id: '<made>' } };
    }
    // the version is the one of the app reading the file
    data.settings.version = '<app>';
    data.firstRundown = Object.keys(data.rundowns)[0];
  }
  // the file as text, so the order of its rundowns survives a reader that sorts keys
  return { name, file: JSON.stringify(input), data };
});
process.stdout.write(JSON.stringify(out));
