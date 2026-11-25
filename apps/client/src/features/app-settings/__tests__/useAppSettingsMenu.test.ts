import { availableOptions, isAvailableHere, staticOptions } from '../useAppSettingsMenu';

const ids = (options: ReturnType<typeof availableOptions>) =>
  options.flatMap((option) => [option.id, ...(option.secondary ?? []).map((child) => child.id)]);

describe('availableOptions', () => {
  it('keeps every menu on a desktop or LAN server', () => {
    expect(availableOptions(staticOptions, false)).toEqual(staticOptions);
  });

  it('hides what the Worker cannot do', () => {
    const shown = ids(availableOptions(staticOptions, true));
    for (const hidden of [
      'settings__mcp',
      'settings__port',
      'settings__custom-views',
      'project',
      'project__create',
      'project__list',
      'manage__sheets',
      'automation',
      'automation__settings',
      'shutdown',
    ]) {
      expect(shown).not.toContain(hidden);
    }
  });

  it('keeps what the Worker serves', () => {
    const shown = ids(availableOptions(staticOptions, true));
    for (const kept of [
      'settings__data',
      'settings__general',
      'settings__aux-timers',
      'settings__view',
      'settings__teleprompter',
      'manage__defaults',
      'manage__custom',
      'manage__rundowns',
      'sharing__link',
      'sharing__report',
      'network__clients',
      'about',
    ]) {
      expect(shown).toContain(kept);
    }
  });

  it('drops a group whose every item is hidden', () => {
    const options = [{ id: 'automation', label: 'A', secondary: [{ id: 'shutdown', label: 'B' }] }];
    expect(availableOptions(options, true)).toEqual([]);
  });
});

describe('isAvailableHere', () => {
  it('agrees with the menu, so a panel hides the same sections', () => {
    expect(isAvailableHere('settings__mcp', true)).toBe(false);
    expect(isAvailableHere('manage__sheets', true)).toBe(false);
    expect(isAvailableHere('settings__general', true)).toBe(true);
    expect(isAvailableHere('settings__mcp', false)).toBe(true);
  });
});
