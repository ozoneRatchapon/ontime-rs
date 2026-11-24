// Runs upstream's wrapText over the cases and prints the wrap fixture.
import { wrapText } from '@upstream/apps/server/src/services/teleprompter-service/teleprompter.wrap.ts';

import { cases } from './wrap-cases.mjs';

const out = cases.map(([name, text, charsPerLine]) => ({
  name,
  text,
  charsPerLine,
  lines: wrapText(text, charsPerLine),
}));
process.stdout.write(JSON.stringify(out));
