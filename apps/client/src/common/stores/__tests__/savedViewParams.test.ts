import { hasCustomParams, stripReservedParams, useSavedViewParams, withStickyParams } from '../savedViewParams';

describe('savedViewParams store', () => {
  beforeEach(() => {
    useSavedViewParams.getState().clearAll();
  });

  it('saves and restores params per view', () => {
    useSavedViewParams.getState().save('timer', 'hideSeconds=true');
    useSavedViewParams.getState().save('backstage', 'showProgress=false');

    expect(useSavedViewParams.getState().params.timer).toBe('hideSeconds=true');
    expect(useSavedViewParams.getState().params.backstage).toBe('showProgress=false');
  });

  it('overwrites the saved params for a view on subsequent saves', () => {
    useSavedViewParams.getState().save('timer', 'hideSeconds=true');
    useSavedViewParams.getState().save('timer', 'hideSeconds=false');

    expect(useSavedViewParams.getState().params.timer).toBe('hideSeconds=false');
  });

  it('does not store an entry for an empty search string', () => {
    useSavedViewParams.getState().save('timer', '');

    expect(useSavedViewParams.getState().params).toEqual({});
  });

  it('removes a previously saved entry when saved with an empty search string', () => {
    useSavedViewParams.getState().save('timer', 'hideSeconds=true');
    useSavedViewParams.getState().save('timer', '');

    expect(useSavedViewParams.getState().params).toEqual({});
  });

  it('ignores empty view keys', () => {
    useSavedViewParams.getState().save('', 'hideSeconds=true');

    expect(useSavedViewParams.getState().params).toEqual({});
  });

  it('clears the saved params for a single view without touching others', () => {
    useSavedViewParams.getState().save('timer', 'hideSeconds=true');
    useSavedViewParams.getState().save('backstage', 'showProgress=false');
    useSavedViewParams.getState().clear('timer');

    expect(useSavedViewParams.getState().params).toEqual({ backstage: 'showProgress=false' });
  });

  it('is a no-op when clearing a view without saved params', () => {
    useSavedViewParams.getState().save('backstage', 'showProgress=false');
    useSavedViewParams.getState().clear('timer');

    expect(useSavedViewParams.getState().params).toEqual({ backstage: 'showProgress=false' });
  });

  it('clears all saved params', () => {
    useSavedViewParams.getState().save('timer', 'hideSeconds=true');
    useSavedViewParams.getState().clearAll();

    expect(useSavedViewParams.getState().params).toEqual({});
  });
});

describe('stripReservedParams', () => {
  it('removes reserved auth/preset params while keeping view customisation', () => {
    expect(stripReservedParams('hideSeconds=true&token=abc&n=1&alias=my')).toBe('hideSeconds=true');
  });

  it('returns an empty string when only reserved params are present', () => {
    expect(stripReservedParams('token=abc&n=1&alias=my')).toBe('');
  });
});

describe('hasCustomParams', () => {
  it('is true when a non-reserved param is present', () => {
    expect(hasCustomParams(new URLSearchParams('hideSeconds=true&token=abc'))).toBe(true);
  });

  it('is false when only reserved params are present', () => {
    expect(hasCustomParams(new URLSearchParams('token=abc&n=1&alias=my'))).toBe(false);
  });

  it('is false when there are no params', () => {
    expect(hasCustomParams(new URLSearchParams(''))).toBe(false);
  });
});

describe('withStickyParams()', () => {
  it('carries the room across to a destination that has none', () => {
    expect(withStickyParams('', '?room=proj-alpha')).toBe('room=proj-alpha');
  });

  it('keeps the destination view params and adds the room', () => {
    const result = new URLSearchParams(withStickyParams('size=2&hidenav=true', '?room=proj-alpha'));
    expect(result.get('size')).toBe('2');
    expect(result.get('hidenav')).toBe('true');
    expect(result.get('room')).toBe('proj-alpha');
  });

  it('does not override a room already set on the destination', () => {
    expect(withStickyParams('room=explicit', '?room=proj-alpha')).toBe('room=explicit');
  });

  it('does nothing when the current location has no room', () => {
    expect(withStickyParams('size=2', '?foo=bar')).toBe('size=2');
    expect(withStickyParams('', '')).toBe('');
  });

  it('does not carry the token, which lives in sessionStorage instead', () => {
    // a token in the address bar leaks through screen shares and history
    expect(withStickyParams('', '?room=proj-alpha&token=secret')).toBe('room=proj-alpha');
  });

  it('does not carry per-view reserved params', () => {
    const result = withStickyParams('', '?room=proj-alpha&n=1&alias=stage');
    expect(result).toBe('room=proj-alpha');
  });
});
