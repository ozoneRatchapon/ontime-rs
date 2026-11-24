import type { ActivityAction, ActivityActor, ActivityLine } from '../api/activity';

/** 'Somchai', 'room password', 'admin' or 'anyone with the link'. */
export function describeActor(actor: ActivityActor): string {
  switch (actor.kind) {
    case 'person':
      return actor.name || 'a removed person';
    case 'roomPassword':
      return 'room password';
    case 'admin':
      return 'admin';
    case 'anyone':
      return 'anyone with the link';
  }
}

const verbs: Record<ActivityAction, string> = {
  start: 'started',
  load: 'loaded',
  pause: 'paused',
  stop: 'stopped',
  reload: 'restarted',
  'add-time': 'changed the time of',
  roll: 'rolled to',
  'entry-add': 'added',
  'entry-edit': 'edited',
  'entry-delete': 'deleted',
  'entry-delete-all': 'deleted every entry in',
  'entry-move': 'moved',
  'entry-swap': 'swapped',
  'entry-renumber': 'renumbered',
  'entry-clone': 'cloned',
  'entry-group': 'grouped',
  'entry-ungroup': 'ungrouped',
  'delay-apply': 'applied the delay',
  'rundown-create': 'created the rundown',
  'rundown-import': 'imported the rundown',
  'rundown-rename': 'renamed a rundown to',
  'rundown-replace': 'replaced the rundown',
  'rundown-delete': 'deleted the rundown',
  'rundown-load': 'loaded the rundown',
  'rundown-duplicate': 'duplicated the rundown',
  'project-data': 'edited the project details',
  settings: 'changed the settings',
  'view-settings': 'changed the view settings',
  'custom-fields': 'changed the custom field',
  'url-presets': 'changed the URL presets',
  css: 'changed the custom CSS',
  'password-set': 'set the room password',
  'password-cleared': 'removed the room password',
  'members-only-on': 'turned on "editing needs sign-in"',
  'members-only-off': 'turned off "editing needs sign-in"',
  restore: 'restored the room from a backup',
  'report-archive': 'archived the report',
  'report-clear': 'cleared the report',
  'person-create': 'added the person',
  'person-remove': 'removed the person',
  'invite-issue': 'made an invite link for',
  'invite-redeem': 'signed in with an invite link',
  'sign-out': 'signed out',
  'members-change': 'changed who looks after the room',
  'operator-add': 'added the operator',
  'operator-remove': 'removed the operator',
  'prompter-play': 'started the teleprompter',
  'prompter-pause': 'paused the teleprompter',
  'prompter-move': 'moved the teleprompter',
  'prompter-speed': 'changed the teleprompter speed',
  'prompter-mode': 'changed how the teleprompter reads',
  'prompter-blackout': 'blacked out or restored the teleprompter screens',
};

/** Teleprompter lines name the event being read, joined by a preposition */
const subjectJoins: Partial<Record<ActivityAction, string>> = {
  'prompter-play': 'at',
  'prompter-pause': 'at',
  'prompter-move': 'to',
  'prompter-speed': 'at',
  'prompter-mode': 'at',
  'prompter-blackout': 'at',
};

/** '+1 min', '-30 s' */
export function formatAmount(ms: number): string {
  const sign = ms < 0 ? '-' : '+';
  const abs = Math.abs(ms);
  if (abs >= 60_000 && abs % 60_000 === 0) return `${sign}${abs / 60_000} min`;
  return `${sign}${Math.round(abs / 1000)} s`;
}

/** What happened, without who or when: 'edited 2.1 Keynote (note) and 2 more'. */
export function describeChange(line: ActivityLine): string {
  const parts = [verbs[line.action] ?? line.action];
  const subject = [line.subject?.cue, line.subject?.title].filter(Boolean).join(' ');
  if (subject) {
    const join = subjectJoins[line.action];
    parts.push(join ? `${join} ${subject}` : subject);
  }
  if (line.amount !== undefined) parts.push(formatAmount(line.amount));
  if (line.fields?.length) parts.push(`(${line.fields.join(', ')})`);
  if (line.more) parts.push(`and ${line.more} more`);
  if (line.room) parts.push(`in ${line.room}`);
  if (line.times && line.times > 1) parts.push(`×${line.times}`);
  return parts.join(' ');
}
