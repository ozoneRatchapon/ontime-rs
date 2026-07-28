import type { MaybeNumber } from '../../utils/utils.type.js';

export type OntimeEventReport = {
  startedAt: MaybeNumber;
  endedAt: MaybeNumber;
};

export type OntimeReport = Record<string, OntimeEventReport>;

/** One event as it actually ran, with its planned title and duration copied in. */
export type ArchivedRunEvent = {
  id: string;
  cue: string;
  title: string;
  /** Planned duration, milliseconds. */
  plannedDuration: number;
  startedAt: MaybeNumber;
  endedAt: MaybeNumber;
};

/** A completed run of one rundown, preserved permanently for later analysis. */
export type ArchivedRun = {
  rundownId: string;
  rundownTitle: string;
  archivedAt: number;
  events: ArchivedRunEvent[];
};
