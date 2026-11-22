#!/usr/bin/env node
// Drive the parity harness in headless Chromium, and read the verdict back.
//
// This is the part of `scripts/browser-parity.sh` that needs a browser. It does
// three jobs, because they are one act: serve the page and the vault over HTTP,
// start Chrome with a debugging port, and speak the Chrome DevTools Protocol to
// read `window.__result` out of the page.
//
// The server sends `Cross-Origin-Opener-Policy: same-origin` and
// `Cross-Origin-Embedder-Policy: require-corp`, which is not optional: the seam's
// synchronous bridge parks a worker on a `SharedArrayBuffer`, and a page that is
// not cross-origin isolated cannot construct one.
//
//   node scripts/drive-browser.mjs <out-dir> [--chrome <path>] [--timeout <s>]
//                                    [--attach <port>] [--verbose]
//
// `--attach` reuses a Chrome already listening on that debugging port instead of
// launching one: the page reloads against whatever is served now, so a debugging
// loop pays the wasm rebuild and not the browser's start-up. `--verbose` loads
// the page with `?verbose`, which makes the page's host log every filesystem
// operation rather than only the refused ones — the difference between "one
// check is wrong" and "here is the operation that produced it".
//
// `<out-dir>` is what `vault-parity` wrote: `native.json`, and one directory per
// vault. The exit status is 0 only when every comparison passed.

import { createServer } from 'node:http';
import { checkExport } from './check-export.mjs';
import { checkCacheOnly } from './check-cache-only.mjs';
import { checkReceipt } from './check-receipt.mjs';
import { checkPreviewReview } from './check-preview-review.mjs';
import { checkScanProgress, checkScanProgressFixture } from './check-scan-progress.mjs';
import { spawn } from 'node:child_process';
import { readFile, mkdtemp, rm, stat } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { extname, join, normalize, resolve, sep } from 'node:path';

// Two pages, two crates: the harness's and the app's, each with its own `web/`.
// The app's page is the shipped one and the harness's reaches sideways into it
// for the seam's JavaScript (`#149`, DEC-S5), so the served tree is `crates/`
// rather than one crate's `web/` — and nothing outside it, so a run's own
// directory and the two pages are all this server may answer with.
const ROOT = resolve(import.meta.dirname, '..');
const PARITY_PAGE = '/crates/lrsalvage-web/web/index.html';
const APP_PAGE = '/crates/lrsalvage-app/web/index.html';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.css': 'text/css; charset=utf-8',
  '.map': 'application/json; charset=utf-8',
};

const CHROME_CANDIDATES = [
  process.env.CHROME,
  '/c/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
].filter(Boolean);

function usage(message) {
  if (message) console.error(message);
  console.error(
    `usage: node scripts/drive-browser.mjs <out-dir> [--chrome <path>] [--timeout <s>] [--attach <port>] [--verbose] [--page <path>] [--check-review|--check-export|--check-receipt|--check-scan-progress|--check-scan-progress-fixture|--check-cache-only]\n` +
      `  --page defaults to ${PARITY_PAGE}; the app's page is ${APP_PAGE}\n` +
      '  --check-scan-progress-fixture needs <out-dir> to hold a vault (fixture.json and its files);\n' +
      '  the other product checks need the native.json a vault-app run writes',
  );
  process.exit(2);
}

const args = process.argv.slice(2);
let out = null;
let chromePath = null;
let timeoutSeconds = 240;
let attachPort = 0;
let verbose = false;
let pagePath = PARITY_PAGE;
let productCheck = null;
/** What the run prints in front of a product check's numbers. */
let productLabel = '';
/** A whole scan over a supplied vault needs no native run to compare against. */
let needsNative = true;
for (let index = 0; index < args.length; index += 1) {
  const arg = args[index];
  if (arg === '--chrome') chromePath = args[++index];
  else if (arg === '--timeout') timeoutSeconds = Number(args[++index]);
  else if (arg === '--attach') attachPort = Number(args[++index]);
  else if (arg === '--verbose') verbose = true;
  else if (arg === '--check-review') productCheck = checkPreviewReview;
  else if (arg === '--check-export') productCheck = checkExport;
  else if (arg === '--check-receipt') productCheck = checkReceipt;
  else if (arg === '--check-scan-progress') productCheck = checkScanProgress;
  else if (arg === '--check-scan-progress-fixture') {
    productCheck = (send) => checkScanProgressFixture(send, out);
    productLabel = 'checkScanProgressFixture';
    needsNative = false;
  }
  else if (arg === '--check-cache-only') productCheck = checkCacheOnly;
  else if (arg === '--page') {
    // A leading slash is optional: `sh` on this machine rewrites an argument that
    // begins with one into a Windows path, so the script passes it without.
    pagePath = `/${String(args[++index] ?? '').replace(/^\/+/, '')}`;
  }
  else if (out === null) out = resolve(arg);
  else usage(`unexpected argument: ${arg}`);
}
if (out === null) usage();
if (productCheck) pagePath = APP_PAGE;

const chrome = chromePath ?? CHROME_CANDIDATES.find((candidate) => existsSync(candidate));
if (!chrome) usage(`no Chrome found; set CHROME to one, or pass --chrome <path>`);
if (needsNative && !existsSync(join(out, 'native.json'))) usage(`${out} holds no native.json`);

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

async function main() {
  const server = await serve(out);
  const port = server.address().port;
  const page = `http://127.0.0.1:${port}${pagePath}${verbose ? '?verbose' : ''}`;
  console.log(`serving ${ROOT} (crates/ only) and ${out} on ${page}`);

  if (attachPort) {
    // A Chrome somebody else started: open a tab on this run's page and drive it.
    // The launch-and-wait path below is skipped entirely, which is the point.
    console.log(`attaching to Chrome on ${attachPort}`);
    const target = await openTab(attachPort, page);
    process.exit(await run(target, timeoutSeconds));
  }

  const profile = await mkdtemp(join(tmpdir(), 'lrsalvage-chrome-'));
  const debugging = 9222 + Math.floor(Math.random() * 500);
  const browser = spawn(
    chrome,
    [
      '--headless=new',
      '--disable-gpu',
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-extensions',
      '--disable-background-networking',
      `--user-data-dir=${profile}`,
      `--remote-debugging-port=${debugging}`,
      page,
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  );
  let browserLog = '';
  browser.stderr.on('data', (chunk) => {
    browserLog += chunk.toString();
  });

  let exitCode = 1;
  try {
    const target = await waitForTarget(debugging);
    exitCode = await run(target, timeoutSeconds);
  } finally {
    browser.kill('SIGKILL');
    server.close();
    await rm(profile, { recursive: true, force: true }).catch(() => {});
    if (exitCode !== 0 && browserLog.trim() !== '') {
      // Chrome's own complaints, last, so they cannot hide the verdict.
      console.error('--- chrome stderr ---');
      console.error(browserLog.trim().split('\n').slice(-40).join('\n'));
    }
  }
  process.exit(exitCode);
}

function serve(root) {
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    const path = decodeURIComponent(url.pathname);
    // `/` is the parity page, which is what this driver was written for; a run
    // that names another page does so with `--page`.
    const file = path === '/' || path === '/index.html' ? PARITY_PAGE.slice(1) : path.slice(1);
    const roots = [ROOT, root];
    for (const base of roots) {
      // The checkout answers only for `crates/`: this server is what serves two
      // pages' modules, and a checkout's `.env` is not one of them.
      if (base === ROOT && !file.startsWith('crates' + sep) && !file.startsWith('crates/')) {
        continue;
      }
      const candidate = normalize(join(base, file));
      if (!candidate.startsWith(base + sep) && candidate !== base) continue;
      try {
        const found = await stat(candidate);
        if (!found.isFile()) continue;
        const body = await readFile(candidate);
        response.writeHead(200, {
          'Content-Type': TYPES[extname(candidate)] ?? 'application/octet-stream',
          'Content-Length': body.length,
          // The bridge's `SharedArrayBuffer` needs an isolated page, and nothing
          // here is cross-origin, so requiring a corpus changes nothing else.
          'Cross-Origin-Opener-Policy': 'same-origin',
          'Cross-Origin-Embedder-Policy': 'require-corp',
          'Cross-Origin-Resource-Policy': 'same-origin',
          'Cache-Control': 'no-store',
        });
        response.end(body);
        return;
      } catch {
        // Not in this root; try the next.
      }
    }
    response.writeHead(404, { 'Content-Type': 'text/plain' });
    response.end(`not found: ${path}\n`);
  });
  return new Promise((ready) => {
    server.listen(0, '127.0.0.1', () => ready(server));
  });
}

/** A tab on `url` in an already-running Chrome, opening one if it has none. */
async function openTab(port, url) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    // `PUT /json/new` is what a recent Chrome wants; a browser without it lists
    // its targets and we navigate the first page instead.
    const opened = await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent(url)}`, {
      method: 'PUT',
    }).catch(() => null);
    if (opened && opened.ok) {
      const target = await opened.json();
      if (target.webSocketDebuggerUrl) return { ...target, url };
    }
    try {
      const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = targets.find((target) => target.type === 'page' && target.webSocketDebuggerUrl);
      if (page) return page;
    } catch {
      // Chrome is not listening yet.
    }
    await sleep(250);
  }
  throw new Error(`no page target on the Chrome at ${port}; is it listening?`);
}

async function waitForTarget(port) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = targets.find(
        (target) => target.type === 'page' && target.url.startsWith('http://127.0.0.1:'),
      );
      if (page && page.webSocketDebuggerUrl) return page;
    } catch {
      // Chrome is not listening yet.
    }
    await sleep(250);
  }
  throw new Error(`Chrome did not offer a page on ${port} within 30 s`);
}

/**
 * Call `window.__gesture()` once, with a user gesture, and leave its answer where
 * the page can read it (`window.__gestureResult`).
 *
 * A page that offers it does so because its first act is a dialog: the app's own
 * page calls `showDirectoryPicker`, and `scripts/browser-parity.sh`'s page offers
 * none, so this is a no-op there. The call is awaited *inside* the gesture, which
 * is what makes the rejection count as the page's.
 */
async function offerGesture(send) {
  // A few seconds, not the whole run: a page that offers no gesture offers none,
  // and waiting out the timeout would spend the run's budget on nothing.
  const until = Date.now() + 5_000;
  for (;;) {
    const offered = await send('Runtime.evaluate', {
      expression: 'typeof window.__gesture === "function" && window.__gestureDone !== true',
      returnByValue: true,
    });
    if (offered.result?.value === true) break;
    if (Date.now() > until) return;
    await sleep(100);
  }
  const picked = await send('Runtime.evaluate', {
    expression:
      'Promise.resolve(window.__gesture()).then((value) => { window.__gestureResult = value; window.__gestureDone = true; return JSON.stringify(value); })',
    awaitPromise: true,
    returnByValue: true,
    userGesture: true,
  });
  if (picked.exceptionDetails) {
    console.log(`[gesture] threw: ${picked.exceptionDetails.text ?? 'no detail'}`);
    return;
  }
  console.log(`[gesture] ${picked.result?.value ?? 'no answer'}`);
}

/** Read `window.__result` out of the page, and report it. */
async function run(target, seconds) {
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  const pending = new Map();
  let nextId = 1;
  let exceptions = [];
  const send = (method, params = {}) =>
    new Promise((resolveSend, rejectSend) => {
      const id = nextId++;
      pending.set(id, { resolveSend, rejectSend, method });
      socket.send(JSON.stringify({ id, method, params }));
    });

  await new Promise((open, fail) => {
    socket.onopen = open;
    socket.onerror = () => fail(new Error(`cannot reach ${target.webSocketDebuggerUrl}`));
  });
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.id !== undefined && pending.has(message.id)) {
      const { resolveSend, rejectSend, method } = pending.get(message.id);
      pending.delete(message.id);
      // The method belongs in the message: "Invalid parameters" from one of forty
      // calls is not a diagnosis.
      if (message.error) rejectSend(new Error(`${method}: ${message.error.message}`));
      else resolveSend(message.result);
      return;
    }
    if (message.method === 'Runtime.exceptionThrown') {
      exceptions.push(message.params.exceptionDetails?.exception?.description ?? 'exception');
    }
    if (message.method === 'Runtime.consoleAPICalled') {
      const text = (message.params.args ?? [])
        .map((arg) => arg.value ?? arg.description ?? '')
        .join(' ');
      if (text.trim() !== '') console.log(`[page] ${text}`);
    }
  };

  await send('Runtime.enable');
  await send('Page.enable');
  if (productCheck) {
    await send('Emulation.setDeviceMetricsOverride', {
      width: 1000, height: 680, deviceScaleFactor: 1, mobile: false,
    });
  }
  await send('Page.navigate', { url: target.url });

  if (productCheck) {
    try {
      await send('Page.bringToFront');
      const numbers = await productCheck(send);
      console.log((productLabel || productCheck.name) + ':', JSON.stringify(numbers));
      if (exceptions.length) throw new Error(exceptions.join('\n'));
      return 0;
    } finally {
      socket.close();
    }
  }

  const deadline = Date.now() + seconds * 1000;

  // One real user gesture, if the page offers one. A File System Access picker
  // refuses without a gesture ("Must be handling a user gesture"), and no headless
  // browser can be handed a dialog to answer — so this is how the *refusal* and the
  // *cancellation* of a picker are driven rather than imitated (`#149`).
  await offerGesture(send);

  let state = null;
  while (Date.now() < deadline) {
    const evaluated = await send('Runtime.evaluate', {
      expression:
        'JSON.stringify({ done: window.__done === true, result: window.__result ?? null, log: (document.getElementById("log") || {}).textContent ?? "" })',
      returnByValue: true,
      awaitPromise: true,
    });
    if (evaluated.exceptionDetails) {
      throw new Error(evaluated.exceptionDetails.text ?? 'the page threw');
    }
    state = JSON.parse(evaluated.result.value);
    if (state.done) break;
    await sleep(500);
  }
  socket.close();

  if (state === null || !state.done) {
    console.error(state?.log ?? '');
    throw new Error(`the harness did not finish within ${seconds} s`);
  }
  if (state.log.trim() !== '') console.log(state.log.trim());

  const result = state.result ?? {};
  const summary = {
    ok: result.ok === true,
    differences: result.differences ?? [],
    error: result.error ?? null,
    numbers: result.numbers ?? null,
  };
  console.log('--- parity result ---');
  console.log(JSON.stringify(summary, null, 2));
  if (exceptions.length > 0) {
    console.error('the page threw:');
    for (const exception of exceptions) console.error(`  ${exception}`);
  }
  return summary.ok && exceptions.length === 0 ? 0 : 1;
}

main().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
