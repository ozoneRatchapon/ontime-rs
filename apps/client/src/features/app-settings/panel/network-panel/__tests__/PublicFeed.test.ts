import { publicUrls } from '../PublicFeed';

describe('publicUrls', () => {
  it('points at the room on the Worker, with no token', () => {
    const urls = publicUrls('https://w.example', 'road to mainnet');
    expect(urls.page).toBe('https://w.example/r/road%20to%20mainnet/public/agenda/');
    expect(urls.feed).toBe('https://w.example/r/road%20to%20mainnet/public');
    expect(urls.page).not.toContain('token');
  });
});
