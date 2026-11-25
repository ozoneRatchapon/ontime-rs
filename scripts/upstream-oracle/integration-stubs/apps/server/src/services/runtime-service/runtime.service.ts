import { record, recorder } from '@record';

const outcome = () => record.ok;
const names = [
  'start',
  'startNext',
  'startPrevious',
  'startById',
  'startByIndex',
  'startByCue',
  'loadNext',
  'loadPrevious',
  'loadById',
  'loadByIndex',
  'loadByCue',
  'pause',
  'stop',
  'reload',
  'roll',
  'addTime',
  'setOffsetMode',
];
export const runtimeService = Object.fromEntries(
  names.map((name) => [name, recorder(`runtimeService.${name}`, outcome)]),
) as Record<string, (...args: unknown[]) => boolean>;
