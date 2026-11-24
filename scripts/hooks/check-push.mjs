#!/usr/bin/env node
// Refuses a push that would publish people's data or a secret.
//
// git runs it as the pre-push hook (scripts/hooks/pre-push), which passes the
// remote's name and URL as arguments and one line per ref on stdin:
//   <local ref> <local sha> <remote ref> <remote sha>
//
// What it looks for, in every commit being pushed (paths and added lines):
//   - a real schedule file: anything in schedules/ but README.md and example.json;
//   - any value from those files that identifies a person: row titles (names),
//     notes and custom values (student ID, topic, advisors), 5+ characters;
//   - the Worker's master password, read from the Keychain on this machine;
//   - text shaped like a private key or a common API token.
// The first two only matter for a remote that is not private; secrets are
// refused everywhere. A match is reported by kind and file, never by value.
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const [remoteName = '', remoteUrl = ''] = process.argv.slice(2);
const root = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
const isPrivate = /-private(\.git)?$/.test(remoteUrl);
const ZERO = /^0+$/;
const ALLOWED_IN_SCHEDULES = new Set(['schedules/README.md', 'schedules/example.json']);

// --- what must not leave ----------------------------------------------------

/** Values that identify people, keyed to the file they came from. */
function personalValues() {
  const dir = join(root, 'schedules');
  if (!existsSync(dir)) return [];
  const found = [];
  for (const name of readdirSync(dir)) {
    if (!name.endsWith('.json') || ALLOWED_IN_SCHEDULES.has(`schedules/${name}`)) continue;
    let schedule;
    try {
      schedule = JSON.parse(readFileSync(join(dir, name), 'utf8'));
    } catch {
      continue;
    }
    for (const row of schedule.rows ?? []) {
      const values = [row.title, row.note, ...Object.values(row.custom ?? {})];
      for (const value of values) {
        if (typeof value === 'string' && value.trim().length >= 5)
          found.push({ value: value.trim(), from: `schedules/${name}` });
      }
    }
  }
  return found;
}

function masterPassword() {
  try {
    const value = execFileSync('security', ['find-generic-password', '-s', 'ontime-session-password', '-w'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    return value.length >= 6 ? value : null;
  } catch {
    return null;
  }
}

const TOKEN_SHAPES = [
  ['a private key', /-----BEGIN [A-Z ]*PRIVATE KEY-----/],
  ['a GitHub token', /\bgh[pousr]_[A-Za-z0-9]{30,}\b/],
  ['an Anthropic or OpenAI key', /\bsk-(ant-)?[A-Za-z0-9_-]{30,}\b/],
  ['an AWS access key', /\bAKIA[0-9A-Z]{16}\b/],
  ['a Cloudflare API token assignment', /CLOUDFLARE_API_TOKEN\s*[=:]\s*['"]?[A-Za-z0-9_-]{30,}/],
];

// --- what is being pushed ---------------------------------------------------

function commitsFor(localSha, remoteSha) {
  if (ZERO.test(localSha)) return []; // a branch delete sends nothing
  const range = ZERO.test(remoteSha) ? [localSha, '--not', `--remotes=${remoteName}`] : [`${remoteSha}..${localSha}`];
  return git('rev-list', ...range)
    .split('\n')
    .filter(Boolean);
}

const refs = readFileSync(0, 'utf8')
  .split('\n')
  .filter(Boolean)
  .map((line) => line.split(' '));
const commits = new Set(refs.flatMap(([, localSha, , remoteSha]) => commitsFor(localSha, remoteSha)));

const problems = [];
const personal = isPrivate ? [] : personalValues();
const password = masterPassword();

for (const commit of commits) {
  const short = commit.slice(0, 8);
  if (!isPrivate) {
    const paths = git('diff-tree', '--no-commit-id', '--name-only', '-r', '--root', commit).split('\n').filter(Boolean);
    for (const path of paths) {
      if (path.startsWith('schedules/') && !ALLOWED_IN_SCHEDULES.has(path))
        problems.push(`${short}: a real schedule file (${path})`);
    }
  }
  const added = git('show', '--format=', '--no-color', '--unified=0', commit)
    .split('\n')
    .filter((line) => line.startsWith('+') && !line.startsWith('+++'))
    .join('\n');
  if (!added) continue;
  for (const { value, from } of personal) {
    if (added.includes(value)) problems.push(`${short}: personal data from ${from}`);
  }
  if (password && added.includes(password)) problems.push(`${short}: the master password`);
  for (const [kind, shape] of TOKEN_SHAPES) {
    if (shape.test(added)) problems.push(`${short}: something shaped like ${kind}`);
  }
}

if (problems.length > 0) {
  console.error(`\npush to ${remoteName} refused — it would publish:`);
  for (const problem of [...new Set(problems)]) console.error(`  - ${problem}`);
  console.error('\nRemove it from those commits first. Nothing was pushed.\n');
  process.exit(1);
}
console.error(`push check: ${commits.size} commit(s) to ${remoteName}${isPrivate ? ' (private)' : ''} — clean`);
