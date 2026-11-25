import { getViewUrl } from '../customViews.utils';

describe('getViewUrl on the Worker', () => {
  it('is under the room on the Worker, with the token of a locked room', () => {
    const url = new URL(getViewUrl('stage-left', true, 'tok'));
    expect(url.pathname.endsWith('/external/stage-left/')).toBe(true);
    expect(url.searchParams.get('token')).toBe('tok');
  });

  it('carries no token for an open room', () => {
    const url = new URL(getViewUrl('stage-left', true, null));
    expect(url.search).toBe('');
  });
});
