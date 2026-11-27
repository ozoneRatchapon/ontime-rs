import { bethereThemeCss } from '../bethereTheme';

// plan 024: the BeThere preset loaded into the CSS override editor
describe('bethereThemeCss', () => {
  const rules = bethereThemeCss.replace(/\/\*[\s\S]*?\*\//g, '').trim();

  test('the font import comes first, or the browser drops it', () => {
    expect(rules.startsWith("@import url('https://fonts.googleapis.com/css2?")).toBe(true);
    expect(rules).toContain('family=Inter');
    expect(rules).toContain('family=Anuphan');
  });

  test('sets the variables the views read, with BeThere colours', () => {
    for (const variable of [
      '--background-color-override',
      '--card-background-color-override',
      '--color-override',
      '--secondary-color-override',
      '--label-color-override',
      '--timer-color-override',
      '--timer-warning-color-override',
      '--timer-danger-color-override',
      '--timer-overtime-color-override',
      '--font-family-override',
    ]) {
      expect(rules).toMatch(new RegExp(`${variable}: [^;]+;`));
    }
    expect(rules).toContain('--background-color-override: #12152a;');
    expect(rules).toContain("'Inter', 'Anuphan'");
  });

  test('every colour is a hex the browser accepts', () => {
    const colours = rules.match(/#[0-9a-z]+(?=;)/gi) ?? [];
    expect(colours.length).toBeGreaterThan(10);
    for (const colour of colours) {
      expect(colour).toMatch(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
    }
  });
});
