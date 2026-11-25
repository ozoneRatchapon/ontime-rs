import { useMemo } from 'react';

import useAppVersion from '../../common/hooks-query/useAppVersion';
import { isDocker, isWorkerBackend } from '../../externals';

export type SettingsOption = {
  id: string;
  label: string;
  secondary?: Readonly<SettingsOption[]>;
  highlight?: string;
};

export const staticOptions = [
  {
    id: 'settings',
    label: 'Settings',
    secondary: [
      { id: 'settings__data', label: 'Project data' },
      { id: 'settings__general', label: 'General settings' },
      { id: 'settings__aux-timers', label: 'Aux timers' },
      { id: 'settings__view', label: 'View settings' },
      { id: 'settings__teleprompter', label: 'Teleprompter' },
      { id: 'settings__custom-views', label: 'Custom views' },
      { id: 'settings__mcp', label: 'MCP Server' },
      ...(isDocker ? [] : [{ id: 'settings__port', label: 'Server port' }]),
    ],
  },
  {
    id: 'project',
    label: 'Project',
    secondary: [
      { id: 'project__create', label: 'Create...' },
      { id: 'project__list', label: 'Manage projects' },
    ],
  },
  {
    id: 'manage',
    label: 'Project settings',
    secondary: [
      { id: 'manage__defaults', label: 'Rundown defaults' },
      { id: 'manage__custom', label: 'Custom fields' },
      { id: 'manage__rundowns', label: 'Manage rundowns' },
      { id: 'manage__sheets', label: 'Import spreadsheet' },
      { id: 'manage__sheets', label: 'Sync with Google Sheet' },
    ],
  },
  {
    id: 'automation',
    label: 'Automation',
    secondary: [
      { id: 'automation__settings', label: 'Automation settings' },
      { id: 'automation__automations', label: 'Manage automations' },
      { id: 'automation__triggers', label: 'Manage triggers' },
    ],
  },
  {
    id: 'sharing',
    label: 'Sharing and reporting',
    secondary: [
      { id: 'sharing__presets', label: 'URL Presets' },
      {
        id: 'sharing__link',
        label: 'Share link',
      },
      { id: 'sharing__report', label: 'Runtime report' },
      { id: 'sharing__archive', label: 'Run archive' },
    ],
  },
  {
    id: 'network',
    label: 'Network',
    secondary: [
      {
        id: 'network__log',
        label: 'Event log',
      },
      {
        id: 'network__clients',
        label: 'Manage clients',
      },
    ],
  },
  {
    id: 'about',
    label: 'About',
  },
  {
    id: 'shutdown',
    label: 'Shutdown',
  },
] as const;

// a child of navigation or a child of secondary navigation
export type SettingsOptionId =
  | (typeof staticOptions)[number]['id']
  | Extract<(typeof staticOptions)[number], { secondary: object }>['secondary'][number]['id'];

/**
 * What the Cloudflare Worker answers 501 for, or has no route for at all: a
 * menu that only leads to an error is hidden rather than shown.
 * - MCP server and server port: there is no local process.
 * - Creating a project: there is no filesystem; a room is the project. Its
 *   Project panel imports and exports the app's project file instead.
 * - Spreadsheet import and Google Sheets sync: no upload handling, no OAuth.
 * - Custom views: uploads are refused.
 * - Automations: stored but never sent, since a Worker cannot keep connections out.
 * - Shutdown: there is nothing to shut down.
 */
const notOnWorker: ReadonlySet<string> = new Set([
  'settings__mcp',
  'settings__port',
  'settings__custom-views',
  // the room has no project list; its Project panel moves files in and out instead
  'project__create',
  'manage__sheets',
  'automation',
  'shutdown',
]);

/**
 * Whether a settings section works on this backend. The panels ask too, since
 * a panel shows all its sections on one page and hiding the menu entry alone
 * leaves the section in view.
 */
export function isAvailableHere(id: SettingsOptionId, onWorker = isWorkerBackend): boolean {
  return !onWorker || !notOnWorker.has(id);
}

/** The menu without what this backend cannot do; a group left empty goes too. */
export function availableOptions(options: Readonly<SettingsOption[]>, onWorker: boolean): SettingsOption[] {
  if (!onWorker) return [...options];
  return options
    .filter((option) => !notOnWorker.has(option.id))
    .map((option) =>
      option.secondary
        ? // a copy, as the highlight below makes one: the option list is shared
          Object.assign({}, option, { secondary: option.secondary.filter((child) => !notOnWorker.has(child.id)) })
        : option,
    )
    .filter((option) => !option.secondary || option.secondary.length > 0);
}

export function useAppSettingsMenu() {
  const { data } = useAppVersion();

  const options: Readonly<SettingsOption[]> = useMemo(
    () =>
      availableOptions(staticOptions, isWorkerBackend).map((option) =>
        // if there is an update then highlight the about setting
        option.id === 'about' && data.hasUpdates
          ? Object.assign({}, option, { highlight: 'New version available' })
          : option,
      ),
    [data],
  );

  return { options };
}
