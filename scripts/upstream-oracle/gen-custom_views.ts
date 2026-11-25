// The custom view contract (plan 021): which names and which pages upstream's
// own customViews.service accepts, and the message it refuses a page with.
// tests/custom_views_tests.rs asks ontime_core::custom_views the same.
import {
  isValidCustomViewSlug,
  validateHtmlContent,
} from '@upstream/apps/server/src/api-data/custom-views/customViews.service.ts';

const slugs = [
  'demo', 'a', 'stage-left', 'obs-2026', '0', 'a'.repeat(63), 'a'.repeat(64), '', '-a', 'a-', 'A', 'a_b',
  'a b', 'a.b', 'ก', 'a--b', '../x', 'x/y',
];

const page = (head: string, body = '') => `<!doctype html><html><head>${head}</head><body>${body}</body></html>`;
const pages: [string, string][] = [
  ['a plain page', page('<style>body{}</style>', '<script>let a = 1;</script>')],
  ['the html tag first', '<html><body>x</body></html>'],
  ['spaces before', '  \n <!DOCTYPE html>\n<html></html>'],
  ['upper case doctype', '<!DOCTYPE HTML><html></html>'],
  ['doctype without a space', '<!doctypehtml><html></html>'],
  ['doctype of something else', '<!doctype svg><svg></svg>'],
  ['text first', 'hello <html></html>'],
  ['a comment first', '<!-- note --><!doctype html><html></html>'],
  ['an html-ish tag', '<htmlx></htmlx>'],
  ['an external script', page('<script src="https://cdn.example/x.js"></script>')],
  ['an external script, spaced', page('<script type="module" src = "x.js"></script>')],
  ['an inline script naming src', page('<script>const src = 1;</script>')],
  ['data-src on a script', page('<script data-src="x"></script>')],
  ['a stylesheet link', page('<link rel="stylesheet" href="x.css">')],
  ['a stylesheet link unquoted', page('<link href=x.css rel=stylesheet>')],
  ['a preconnect link', page('<link rel="preconnect" href="https://fonts.example">')],
  ['an icon link', page('<link rel="icon" href="data:,">')],
  ['an iframe', page('', '<iframe src="x"></iframe>')],
  ['an iframe tag with no space', page('', '<iframe>')],
  ['an iframe-like word', page('', '<p>use <iframes> carefully</p>')],
  ['an image from anywhere', page('', '<img src="https://example.com/logo.png">')],
  ['upper case script', page('<SCRIPT SRC="x.js"></SCRIPT>')],
];

const message = (run: () => void) => {
  try {
    run();
    return null;
  } catch (error) {
    return (error as Error).message;
  }
};

process.stdout.write(
  JSON.stringify({
    slugs: slugs.map((slug) => ({ slug, valid: isValidCustomViewSlug(slug) })),
    pages: pages.map(([name, html]) => ({ name, html, refused: message(() => validateHtmlContent(html)) })),
  }),
);
