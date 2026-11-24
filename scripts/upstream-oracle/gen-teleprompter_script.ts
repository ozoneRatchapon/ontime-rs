// The script and settings contract: buildScriptEvents, parseTeleprompterSettings,
// isReadableScriptSource and renameScriptSource, run over our cases.
import {
  buildScriptEvents,
  isReadableScriptSource,
  parseTeleprompterSettings,
} from '@upstream/apps/server/src/services/teleprompter-service/teleprompter.utils.ts';
import {
  defaultTeleprompterSettings,
  renameScriptSource,
} from '@upstream/packages/utils/src/teleprompter/teleprompterSettings.ts';

const event = (id: string, patch: Record<string, unknown> = {}) => ({
  id,
  type: 'event',
  cue: id.toUpperCase(),
  title: `Title ${id}`,
  note: `Note ${id}`,
  custom: { script: `Script ${id}` },
  skip: false,
  parent: null,
  ...patch,
});
const rundown = (entries: Record<string, unknown>[], flatOrder = entries.map((e) => e.id as string)) => ({
  id: 'show',
  title: 'Show',
  order: flatOrder,
  flatOrder,
  entries: Object.fromEntries(entries.map((e) => [e.id, e])),
  revision: 1,
});
const customFields = {
  script: { type: 'text', label: 'Script', colour: '' },
  poster: { type: 'image', label: 'Poster', colour: '' },
};
const settings = (patch: Record<string, unknown> = {}) => ({ ...defaultTeleprompterSettings, ...patch });

const builds: [string, ReturnType<typeof rundown>, ReturnType<typeof settings>][] = [
  ['reads the note', rundown([event('a')]), settings({ script: 'note', heading: 'none' })],
  ['reads the title', rundown([event('a')]), settings({ script: 'title', heading: 'none' })],
  ['reads a text custom field', rundown([event('a')]), settings({ script: 'custom-script', heading: 'none' })],
  ['an image field gives nothing', rundown([event('a')]), settings({ script: 'custom-poster' })],
  ['a missing field gives nothing', rundown([event('a')]), settings({ script: 'custom-missing' })],
  ['an unknown source gives nothing', rundown([event('a')]), settings({ script: 'skip' })],
  ['none gives nothing', rundown([event('a')]), settings({ script: 'none' })],
  ['an inherited name gives nothing', rundown([event('a')]), settings({ script: 'custom-constructor' })],
  [
    'rundown order, no skipped events, no other entries',
    rundown([
      event('b'),
      event('skipped', { skip: true }),
      { id: 'delay', type: 'delay', duration: 0 },
      { id: 'ms', type: 'milestone', cue: 'M', title: 'Mile' },
      event('a'),
    ]),
    settings(),
  ],
  ['the title heading', rundown([event('a')]), settings({ heading: 'title' })],
  ['the cue heading', rundown([event('a')]), settings({ heading: 'cue' })],
  ['both headings', rundown([event('a')]), settings({ heading: 'both' })],
  ['both headings without a cue', rundown([event('a', { cue: '' })]), settings({ heading: 'both' })],
  ['no cue, no cue heading', rundown([event('a', { cue: '' })]), settings({ heading: 'cue' })],
  [
    'indentation and positions',
    rundown([event('a', { note: 'one\n  two three' })]),
    settings({ heading: 'none', charsPerLine: 12 }),
  ],
  [
    'events without text are left out',
    rundown([event('a', { note: ' \n ' }), event('b', { note: undefined, custom: undefined }), event('c')]),
    settings(),
  ],
  [
    'a custom field missing on the event',
    rundown([event('a', { custom: {} }), event('b')]),
    settings({ script: 'custom-script' }),
  ],
  ['a non-text value is no text', rundown([event('a', { note: 42 }), event('b')]), settings()],
  ['flat order decides, groups included', rundown([event('a'), event('b'), event('c')], ['c', 'a', 'b']), settings()],
  [
    'long notes wrap',
    rundown([
      event('a', { note: 'Good evening and welcome to the 2026 awards, our first guest is a well-known face' }),
    ]),
    settings({ charsPerLine: 20 }),
  ],
  [
    'thai',
    rundown([event('a', { title: 'เปิดงาน', cue: '1', note: 'สวัสดีครับ ยินดีต้อนรับทุกท่าน' })]),
    settings({ charsPerLine: 10, heading: 'both' }),
  ],
  ['an empty rundown', rundown([]), settings()],
];

const parses: [string, unknown, unknown][] = [
  ['takes what it is given', { script: 'title', charsPerLine: 40, heading: 'none' }, defaultTeleprompterSettings],
  ['clamps a long line', { charsPerLine: 1000 }, defaultTeleprompterSettings],
  ['clamps a short line', { charsPerLine: 2 }, defaultTeleprompterSettings],
  ['rounds a half up', { charsPerLine: 32.5 }, defaultTeleprompterSettings],
  ['rounds down', { charsPerLine: 32.4 }, defaultTeleprompterSettings],
  ['falls back on invalid values', { charsPerLine: 'many', heading: 'banner' }, defaultTeleprompterSettings],
  [
    'a partial patch keeps the rest',
    { heading: 'cue' },
    { script: 'custom-script', charsPerLine: 50, heading: 'both' },
  ],
  ['not an object', 'nope', defaultTeleprompterSettings],
  ['null', null, { script: 'title', charsPerLine: 20, heading: 'none' }],
  ['an array', ['title'], defaultTeleprompterSettings],
  ['infinity', { charsPerLine: Infinity }, defaultTeleprompterSettings],
  ['a numeric script is ignored', { script: 7 }, defaultTeleprompterSettings],
];

const readables = [
  'note',
  'title',
  'custom-script',
  'custom-poster',
  'custom-missing',
  'custom-constructor',
  'skip',
  '',
  'custom-',
];

const renames: [string, string, string | null][] = [
  ['custom-script', 'script', 'lines'],
  ['custom-script', 'script', null],
  ['custom-script', 'other', 'lines'],
  ['note', 'script', 'lines'],
  ['title', 'script', null],
];

process.stdout.write(
  JSON.stringify({
    builds: builds.map(([name, rd, s]) => ({
      name,
      rundown: rd,
      customFields,
      settings: s,
      events: buildScriptEvents(rd as never, customFields as never, s as never),
    })),
    parses: parses.map(([name, data, fallback]) => ({
      name,
      data,
      fallback,
      result: parseTeleprompterSettings(data, fallback as never),
    })),
    readables: readables.map((script) => ({
      script,
      customFields,
      readable: isReadableScriptSource(script, customFields as never),
    })),
    renames: renames.map(([source, oldKey, newKey]) => ({
      source,
      oldKey,
      newKey,
      result: renameScriptSource(source, oldKey, newKey),
    })),
  }),
);
