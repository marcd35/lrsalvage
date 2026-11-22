#!/usr/bin/env node
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import { release, version } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { installWriterProbe } from './writer-scale-probe.mjs';

const root = resolve(import.meta.dirname, '..');
const total = 5000;
const targetMs = 120_000;

function git(...args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
}

function cargo(...args) {
  execFileSync('cargo', args, { cwd: root, stdio: 'inherit' });
}

async function until(probe, detail, seconds = 600) {
  const deadline = performance.now() + seconds * 1000;
  do {
    const result = await probe();
    if (result) return result;
    await sleep(150);
  } while (performance.now() < deadline);
  throw new DOMException(detail, 'TimeoutError');
}

async function waitForWrites(evaluate, record, detail) {
  let previousCount = 0;
  let lastProgress = performance.now();
  for (;;) {
    const state = await evaluate('writerScale.state()');
    record(state);
    if (state.settled) return;
    const events = state.writer_opens + state.jpeg_closes;
    if (events !== previousCount) {
      previousCount = events;
      lastProgress = performance.now();
    } else if (performance.now() - lastProgress > 600_000) {
      throw new DOMException(detail, 'TimeoutError');
    }
    await sleep(150);
  }
}

class WriterAssertionError extends Error {}

function verifyWriter(check) {
  try {
    check();
  } catch (error) {
    if (error.code !== 'ERR_ASSERTION') throw error;
    throw new WriterAssertionError(error.message, { cause: error });
  }
}

function recordBrowserEvidence(evidence, stop, state) {
  const { visible_saved_count, writer_opens, jpeg_closes, elapsed_ms, longest_gap_ms } = state;
  evidence[stop ? 'continued' : 'uninterrupted'] = { visible_saved_count, writer_opens, jpeg_closes };
  if (!stop) {
    evidence.performance = { jpeg_closes, elapsed_ms, longest_gap_ms, target_ms: targetMs,
      target_met: jpeg_closes === total ? elapsed_ms <= targetMs : null };
  }
}

async function jpegTree(rootPath, relative = '', files = new Map()) {
  for (const file of await readdir(join(rootPath, relative), { withFileTypes: true })) {
    const path = relative + file.name;
    if (file.isDirectory()) await jpegTree(rootPath, `${path}/`, files);
    else if (/\.jpe?g$/i.test(file.name)) {
      assert(file.isFile(), `expected an ordinary JPEG at ${path}`);
      const bytes = await readFile(join(rootPath, path));
      files.set(path, createHash('sha256').update(bytes).digest('hex'));
    }
  }
  return files;
}

async function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  let id = 0;
  await new Promise((ready, fail) => {
    socket.addEventListener('open', ready, { once: true });
    socket.addEventListener('error', () => fail(new Error('Chrome DevTools connection failed')), { once: true });
  });
  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    const callback = pending.get(message.id);
    if (!callback) return;
    pending.delete(message.id);
    clearTimeout(callback.timeout);
    if (message.error) callback.reject(new Error(message.error.message));
    else callback.resolve(message.result);
  });
  socket.addEventListener('close', () => {
    for (const { reject, timeout } of pending.values()) {
      clearTimeout(timeout);
      reject(new Error('Chrome DevTools disconnected'));
    }
    pending.clear();
  });
  async function evaluate(expression) {
    assert.equal(socket.readyState, WebSocket.OPEN, 'Chrome DevTools disconnected');
    const result = await new Promise((resolve, reject) => {
      const request = ++id;
      const timeout = setTimeout(() => {
        pending.delete(request);
        reject(new DOMException('Chrome DevTools did not answer within 30 seconds', 'TimeoutError'));
      }, 30_000);
      pending.set(request, { resolve, reject, timeout });
      socket.send(JSON.stringify({ id: request, method: 'Runtime.evaluate', params: {
        expression, returnByValue: true, awaitPromise: true,
      } }));
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    }
    return result.result.value;
  }
  return { evaluate, close: () => socket.close() };
}

async function responseJson(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(10_000) });
  assert(response.ok, `HTTP ${response.status} at ${url}`);
  return response.json();
}

function checkReceipt(snapshot, count, destination) {
  const ungrouped = (text) => text.replaceAll(',', '');
  assert.match(ungrouped(snapshot.headline), new RegExp(`^${count} photos? saved$`));
  assert.match(ungrouped(snapshot.detail), new RegExp(`^${count} files? in the folder$`));
  assert.match(snapshot.reconciled, /Every file above was found in the folder/);
  assert.equal(snapshot.destination, `Saved to \u201c${basename(destination)}\u201d`, 'receipt names the wrong folder');
  assert.equal(snapshot.failures, 0, 'the receipt contains photo failures');
}

async function runBatch({ devtools, url, build, fixture, expected, destination, stop }, evidence) {
  const response = await fetch(`${devtools}/json/new?${encodeURIComponent(url)}`, {
    method: 'PUT', signal: AbortSignal.timeout(10_000),
  });
  assert(response.ok, 'Chrome could not open the recovery page');
  const tab = await response.json();
  const page = await connect(tab.webSocketDebuggerUrl);
  const evaluate = page.evaluate;
  const record = (state) => recordBrowserEvidence(evidence, stop, state);
  try {
    await until(() => evaluate(`location.href === ${JSON.stringify(url)} &&
      Boolean(document.getElementById('export-start'))`), 'the hosted recovery page did not load', 30);
    const loaded = await evaluate(`(async () => ({
      isolated: window.isSecureContext && window.crossOriginIsolated,
      build: await (await fetch('/build.json', { cache: 'no-store' })).json(),
    }))()`);
    assert(loaded.isolated, 'the hosted page is not cross-origin isolated');
    assert.deepEqual(loaded.build, build, 'the hosted build changed during verification');
    await evaluate(`window.writerScale = (${installWriterProbe})(${JSON.stringify(basename(destination))}, ${stop}, ${total}); true`);
    console.log(`\n${stop ? 'Stop and continuation' : 'Uninterrupted'} run in Chrome.\n` +
      `1. Choose the catalog folder: ${dirname(fixture.catalog)}\n` +
      `2. Choose the preview folder: ${fixture.cache}\n` +
      `3. Scan, then choose the destination: ${destination}\n` +
      'The command starts saving after the destination is accepted.');
    await until(() => evaluate('writerScale.ready()'), 'could not start with the expected destination and 5,000 mirrored JPEGs');
    await evaluate('writerScale.start()');
    await waitForWrites(evaluate, record, 'no browser writer activity for ten minutes in the first phase');
    let snapshot = await evaluate('writerScale.snapshot()');

    verifyWriter(() => assert(snapshot.progress > 0 && snapshot.progress < total,
      'no intermediate saved count was visible'));
    if (stop) {
      const boundary = snapshot.stopped;
      const committed = snapshot.commits.map(([path]) => path);
      const partial = await jpegTree(destination);
      verifyWriter(() => {
        assert(boundary, 'the eight active streams never reached the stop boundary');
        assert.match(snapshot.status, /Stopped/);
        assert.equal(boundary.inFlight.length, 8);
        assert.equal(snapshot.opens.length, boundary.opened, 'a new writer opened after Stop');
        assert.deepEqual(new Set(committed.slice(boundary.committed)),
          new Set(boundary.inFlight), 'only the held writes may commit after Stop');
        assert(committed.length > 8 && committed.length < total, 'Stop must leave work pending');
        assert.deepEqual(partial, new Map(committed.map((path) => [path, expected.get(path)])),
          'the stopped folder must contain exactly the committed CLI JPEGs');
        checkReceipt(snapshot, partial.size, destination);
      });
      evidence.stop = { before_stop: boundary.committed, settled_after_stop: partial.size,
        in_flight_at_stop: boundary.inFlight.length, visible_saved_count: snapshot.progress };
      await evaluate('writerScale.continue()');
      await waitForWrites(evaluate, record, 'no browser writer activity for ten minutes during continuation');
      snapshot = await evaluate('writerScale.snapshot()');
      verifyWriter(() => assert(snapshot.opens.slice(boundary.opened).every((path) => !partial.has(path)),
        'continuation reopened a JPEG that was already saved'));
    }

    const actual = await jpegTree(destination);
    evidence[stop ? 'continued' : 'uninterrupted'] = {
      visible_saved_count: snapshot.progress, writer_opens: snapshot.opens.length,
      jpeg_closes: snapshot.commits.length, disk_jpegs: actual.size,
      write_failures: snapshot.failures,
      missing: [...expected.keys()].filter((path) => !actual.has(path)).length,
      extra: [...actual.keys()].filter((path) => !expected.has(path)).length,
      wrong: [...actual].filter(([path, hash]) => expected.has(path) && expected.get(path) !== hash).length,
    };
    verifyWriter(() => {
      assert.equal(snapshot.status, `${total} saved of ${total}`);
      assert.equal(snapshot.opens.length, total, 'expected exactly one writer open per JPEG');
      assert.deepEqual(new Set(snapshot.opens), new Set(expected.keys()), 'unexpected JPEG writer paths');
      assert.equal(snapshot.commits.length, total, 'expected exactly one successful close per JPEG');
      assert.deepEqual(new Set(snapshot.commits.map(([path]) => path)), new Set(expected.keys()),
        'unexpected JPEG close paths');
      assert.deepEqual(actual, expected, 'relative JPEG paths and SHA-256 must match the CLI');
      checkReceipt(snapshot, total, destination);
    });
  } finally {
    page.close();
    await fetch(`${devtools}/json/close/${tab.id}`, { signal: AbortSignal.timeout(5_000) }).catch(() => {});
  }
}

async function main() {
  const [candidate, output, storage, ...extra] = process.argv.slice(2);
  if (!candidate || !output || !storage || extra.length !== 0) {
    throw new Error('usage: node scripts/check-writer-scale.mjs <HTTPS candidate /recovery URL> <new work directory> <destination storage description>');
  }
  assert.equal(process.platform, 'win32', 'run this release check on Windows Chrome');
  const url = new URL(candidate);
  assert(url.protocol === 'https:' && url.pathname === '/recovery', 'supply the hosted HTTPS /recovery URL');
  const work = resolve(output);
  assert(!git('status', '--porcelain', '--untracked-files=all'), 'use a clean checkout for release evidence');
  await mkdir(dirname(work), { recursive: true });
  await mkdir(work);
  const evidence = {
    date: new Date().toISOString(), url: url.href,
    windows: { version: version(), release: release() }, storage, correctness: 'inconclusive',
  };
  let browser;
  try {
    const chrome = process.env.CHROME ?? [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA]
      .filter(Boolean).map((directory) => join(directory, 'Google', 'Chrome', 'Application', 'chrome.exe'))
      .find(existsSync);
    assert(chrome && existsSync(chrome), 'set CHROME to the Windows Chrome executable');
    const build = await responseJson(new URL('/build.json', url));
    evidence.build = build;
    assert.equal(build.commit, git('rev-parse', 'HEAD'), 'use the same checkout as the hosted candidate');
    assert.equal(build.dirty, false, 'the hosted candidate was built from a dirty checkout');
    const vault = join(work, 'vault');
    cargo('run', '--locked', '--release', '-p', 'lrsalvage-app', '--bin', 'vault-writer-scale', '--', vault);
    const manifest = JSON.parse(await readFile(join(vault, 'fixture.json'), 'utf8'));
    const fixture = { catalog: join(vault, manifest.catalog), cache: join(vault, manifest.cache) };
    evidence.workload = {
      payloads_by_size: manifest.payloads_by_size,
      payload_bytes: Object.entries(manifest.payloads_by_size).reduce((sum, [size, count]) => sum + Number(size) * count, 0),
    };
    const cli = join(work, 'cli');
    cargo('run', '--locked', '--release', '-p', 'lrsalvage-cli', '--', 'extract',
      '--catalog', fixture.catalog, '--previews', fixture.cache, '--out', cli, '--no-timestamps');
    const expected = await jpegTree(cli);
    assert.equal(expected.size, total, 'CLI reference must contain 5,000 JPEGs');
    assert.equal(new Set(expected.values()).size, total, 'each CLI JPEG must have distinct bytes');

    const profile = join(work, 'chrome-profile');
    browser = spawn(chrome, [`--user-data-dir=${profile}`, '--remote-debugging-port=0',
      '--no-first-run', '--no-default-browser-check', 'about:blank'], { stdio: 'ignore' });
    let launchError;
    browser.once('error', (error) => { launchError = error; });
    const portFile = join(profile, 'DevToolsActivePort');
    const port = await until(async () => {
      if (launchError) throw launchError;
      return (await readFile(portFile, 'utf8').catch(() => '')).split(/\r?\n/)[0];
    }, 'Chrome did not start DevTools', 30);
    const devtools = `http://127.0.0.1:${port}`;
    evidence.chrome = (await responseJson(`${devtools}/json/version`)).Browser;
    assert.match(evidence.chrome, /^Chrome\//, 'the qualifying browser must be Chrome');
    for (const stop of [false, true]) {
      const destination = await mkdtemp(join(work, stop ? 'browser-continued-' : 'browser-uninterrupted-'));
      await runBatch({ devtools, url: url.href, build, fixture, expected, destination, stop }, evidence);
    }
    evidence.correctness = 'passed';
  } catch (error) {
    if (error instanceof WriterAssertionError) evidence.correctness = 'failed';
    evidence.failure = error.message.split('\n')[0];
    throw error;
  } finally {
    browser?.kill();
    const path = join(work, 'writer-scale-result.json');
    await writeFile(path, `${JSON.stringify(evidence, null, 2)}\n`, { flag: 'wx' });
    console.log(`Evidence: ${path}`);
  }
}

await main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
