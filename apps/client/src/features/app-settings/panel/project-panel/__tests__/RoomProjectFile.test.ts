import { projectFileName } from '../RoomProjectFile';

describe('projectFileName', () => {
  it('names the file after the project', () => {
    expect(projectFileName('Gala 2026', 'raknong')).toBe('Gala 2026');
  });

  it('falls back to the room when the project has no title', () => {
    expect(projectFileName('  ', 'raknong')).toBe('raknong');
  });

  it('keeps characters a file name cannot have out of it', () => {
    expect(projectFileName('A/B: "C"?', 'room')).toBe('A-B- -C-');
  });
});
