import { describeActor, describeChange, formatAmount } from '../activityText';

describe('describeActor()', () => {
  it('names a person, or says how the change was allowed', () => {
    expect(describeActor({ kind: 'person', id: 'x', name: 'Somchai' })).toBe('Somchai');
    expect(describeActor({ kind: 'roomPassword' })).toBe('room password');
    expect(describeActor({ kind: 'admin' })).toBe('admin');
    expect(describeActor({ kind: 'anyone' })).toBe('anyone with the link');
  });
});

describe('describeChange()', () => {
  it('names the event by cue and title', () => {
    expect(
      describeChange({
        at: 0,
        actor: { kind: 'admin' },
        action: 'start',
        subject: { cue: '2.1', title: 'Keynote' },
      }),
    ).toBe('started 2.1 Keynote');
  });

  it('lists the changed fields and how many more entries', () => {
    expect(
      describeChange({
        at: 0,
        actor: { kind: 'admin' },
        action: 'entry-edit',
        subject: { title: 'Keynote' },
        fields: ['note', 'custom.camera'],
        more: 2,
      }),
    ).toBe('edited Keynote (note, custom.camera) and 2 more');
  });

  it('shows added time and merged repeats', () => {
    expect(
      describeChange({
        at: 0,
        actor: { kind: 'admin' },
        action: 'add-time',
        amount: 60_000,
        times: 2,
        subject: { cue: '1' },
      }),
    ).toBe('changed the time of 1 +1 min ×2');
  });

  it('reads well with no subject', () => {
    expect(
      describeChange({
        at: 0,
        actor: { kind: 'admin' },
        action: 'password-set',
      }),
    ).toBe('set the room password');
  });
});

describe('formatAmount()', () => {
  it('uses minutes when whole, seconds otherwise', () => {
    expect(formatAmount(60_000)).toBe('+1 min');
    expect(formatAmount(-120_000)).toBe('-2 min');
    expect(formatAmount(-30_000)).toBe('-30 s');
  });
});
