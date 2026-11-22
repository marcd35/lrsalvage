import assert from 'node:assert/strict';
import { setTimeout as wait } from 'node:timers/promises';

// Drive index.html without a product test hook. Only the accepted picker
// answer is supplied. Chrome decodes the JPEGs and handles native focus/Escape.
export async function checkPreviewReview(send) {
  async function evaluate(expression) {
    const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    }
    return result.result.value;
  }

  async function until(expression, detail) {
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      const value = await evaluate(expression);
      if (value) return value;
      await wait(75);
    }
    throw new Error(detail + ' (' + await evaluate(
      '["#photo-count", "#picker-error", "#headline", "#stage", "#photo-dialog-title", "#photo-dialog-note", "#photo-dialog-image"].map(s => document.querySelector(s)?.textContent).join(" | ")'
    ) + ')');
  }

  const lookup = (selector) => 'document.querySelector(' + JSON.stringify(selector) + ')';
  const element = (id) => lookup('#photo-' + id);
  const button = (name) => lookup('button[aria-label="Preview ' + name + '"]');
  const checkbox = (name) => lookup('input[aria-label="Include ' + name + '"]');
  const click = (expression) => evaluate(expression + '.click()');
  const text = (expression) => evaluate(expression + '.textContent.trim()');
  const count = () => text(element('count'));
  const set = (id, value, type = 'change') => evaluate(
    element(id) + '.value = ' + JSON.stringify(value) + '; ' +
    element(id) + '.dispatchEvent(new Event(' + JSON.stringify(type) + ', {bubbles:true}))'
  );

  async function key(name, code) {
    const data = { key: name, code: name, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code };
    const down = name === 'Enter' ? { type: 'keyDown', text: '\r', unmodifiedText: '\r' } : { type: 'rawKeyDown' };
    await send('Input.dispatchKeyEvent', { ...down, ...data });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', ...data });
  }

  await until(lookup('#source') + '?.hidden === false', 'The source screen did not load');
  await evaluate(`(async () => {
    const { json, mountVault, writeFile } =
      await import(new URL('./mount.js', location.href));
    const root = await navigator.storage.getDirectory();
    const review = (await json('/native.json')).vaults.find(vault => vault.name === 'review');
    const fixture = await mountVault(root, 'review-test', '/review', review.files);
    const originals = await mountVault(root, 'review-originals', '/review/originals', review.originals);
    const directory = await fixture.getDirectoryHandle(review.catalog.split('/')[0]);
    const cache = await fixture.getDirectoryHandle(review.cache);
    globalThis.showDirectoryPicker = async ({ id }) => ({
      'lrsalvage-originals': originals, 'lrsalvage-cache': cache,
    }[id] ?? directory);
    const bridgePayload = review.files.find(path => path.includes('AAAA0002-'));
    const meadowPayload = review.files.find(path => path.includes('AAAA0001-'));
    globalThis.replaceBridgePayload = async (replace) => {
      const path = replace ? meadowPayload : bridgePayload;
      const response = await fetch('/review/' + path.split('/').map(encodeURIComponent).join('/'));
      if (!response.ok) throw new Error('Could not fetch review payload: ' + response.status);
      await writeFile(fixture, bridgePayload, new Uint8Array(await response.arrayBuffer()));
    };
    let pending = 0;
    globalThis.maxBackgroundPreviewReads = 0;
    const NativeWorker = globalThis.Worker;
    globalThis.Worker = class extends NativeWorker {
      constructor(...args) {
        super(...args);
        this.addEventListener('message', ({ data }) => {
          if (data.type === 'preview') pending--;
        });
      }
      postMessage(message, ...args) {
        if (message.type === 'preview') {
          pending++;
          if (!document.getElementById('photo-dialog').open &&
              document.getElementById('photo-pane').hidden) {
            globalThis.maxBackgroundPreviewReads = Math.max(pending, globalThis.maxBackgroundPreviewReads);
          }
        }
        super.postMessage(message, ...args);
      }
    };
    globalThis.reviewHashes = async () => {
      const hashes = [];
      for (const path of review.files) {
        const parts = path.split('/');
        const filename = parts.pop();
        let parent = fixture;
        for (const name of parts) parent = await parent.getDirectoryHandle(name);
        const file = await (await parent.getFileHandle(filename)).getFile();
        const digest = await crypto.subtle.digest('SHA-256', await file.arrayBuffer());
        hashes.push(Array.from(new Uint8Array(digest)).join(','));
      }
      return hashes;
    };
  })()`);
  const sourceHashes = await evaluate('reviewHashes()');

  async function scan(withCache = true) {
    await click(lookup('#pick'));
    await until(lookup('#scan') + '?.disabled === false', 'The picked catalog could not be scanned');
    if (withCache) {
      await click(lookup('#pick-cache'));
      await until(lookup('#picked') + '.textContent.includes("(itself)")', 'The split cache was not attached');
    }
    await click(lookup('#scan'));
    await until(lookup('#result') + '?.hidden === false', 'The review screen did not open');
  }
  await scan();
  assert.match(await count(), /10 shown . 6 checked/);
  assert.equal(await evaluate(checkbox('Bridge.CR2') + '.checked'), true);
  assert.equal(await evaluate(checkbox('Creek.CR2') + '.checked'), false);
  assert.match(await text(element('groups')), /Small previews.*Creek.CR2/s);
  // `#192`: the floor `plan` applied is stated where the unticked previews are.
  assert.match(await text(element('groups')),
    /Small previewsThese previews are smaller than the 640 px long edge, so they start unchecked/s);
  assert.match(await text(element('groups')), /Photos without a recoverable preview.*Missing file.CR2/s);
  assert.match(await text(element('groups')), /Missing index entry.CR2No entry in the preview index/);
  const bridge = button('Bridge.CR2') + '.querySelector("img")';
  assert.equal(await until(bridge + '?.naturalWidth', 'The 640 px tile did not decode'), 640);
  const backgroundReads = await evaluate('maxBackgroundPreviewReads');
  assert.equal(backgroundReads, 2);

  await evaluate('replaceBridgePayload(true)');
  await click(button('Bridge.CR2'));
  await until(element('dialog-note') + '.textContent.includes("at scan time")',
    'The changed cache payload did not report its dimensions');
  assert.match(await text(element('dialog-note')), /was 640 . 480 px at scan time. It is now 900 . 600 px/);
  await key('Escape', 27);
  await until('document.activeElement === ' + button('Bridge.CR2'), 'Focus did not return after preview failure');
  await evaluate('replaceBridgePayload(false)');

  await click(element('group'));
  assert.match(await text(element('groups')), /Unknown folder/);
  const folder = lookup('#photo-groups details summary');
  assert.match(await text(folder), /2023/);
  await click(folder);
  await until(element('count') + '.textContent.includes("7 shown")', 'The folder did not collapse');
  assert.match(await count(), /7 shown . 6 checked/);
  await click(element('group'));
  await set('search', 'Creek', 'input');
  assert.match(await count(), /1 shown . 6 checked/);
  await click(element('check-visible'));
  assert.match(await count(), /1 shown . 7 checked/);
  await click(element('uncheck-visible'));
  assert.match(await count(), /1 shown . 6 checked/);
  await set('search', '2024', 'input');
  assert.match(await count(), /4 shown . 6 checked/);
  assert.equal(await evaluate(checkbox('Bridge.CR2')), null);
  await set('search', '', 'input');

  await set('sort', 'size');
  assert.equal(await evaluate(lookup('.review-open') + '.getAttribute("aria-label")'), 'Preview Broken.CR2');
  await set('sort', 'folder');
  assert.equal(await evaluate(lookup('.review-open') + '.getAttribute("aria-label")'), 'Preview Bridge.CR2');
  await set('sort', 'name');
  await set('layout', 'List');
  assert.equal(await evaluate(element('groups') + '.classList.contains("as-list")'), true);
  await set('thumbnails', 'small');
  const small = await evaluate(button('Bridge.CR2') + '.querySelector(".thumb").clientHeight');
  await set('thumbnails', 'large');
  const large = await evaluate(button('Bridge.CR2') + '.querySelector(".thumb").clientHeight');
  assert.ok(large > small, 'Changing thumbnail size should enlarge the list thumbnail');

  await click(element('pane-toggle'));
  await click(button('Portrait.CR2'));
  assert.equal(await text(element('pane-name')), 'Portrait.CR2');
  const paneImage = lookup('#photo-pane-image img');
  assert.equal(await until(paneImage + '?.naturalHeight', 'The portrait pane did not decode'), 800);
  await evaluate(element('enlarge') + '.focus()');
  assert.equal(await evaluate('document.activeElement.id'), 'photo-enlarge');
  await key('Enter', 13);
  await until(element('dialog') + '.open', 'Enter did not open the enlarged preview');
  assert.match(await text(element('dialog-size')), /800 . 512 px . 90/);
  const dialogImage = lookup('#photo-dialog-image img');
  assert.equal(await until(dialogImage + '?.naturalWidth', 'The rotated portrait did not decode'), 512);
  assert.equal(await evaluate(dialogImage + '.naturalHeight'), 800);
  const fitH = await evaluate(dialogImage + '.clientHeight');
  await click(element('actual'));
  const actualH = await evaluate(dialogImage + '.clientHeight');
  assert.equal(actualH, 800);
  assert.ok(fitH < actualH, 'Fit must contain the preview in the small viewport');
  assert.ok(await evaluate(element('dialog-image') + '.scrollHeight > ' + element('dialog-image') + '.clientHeight'));
  await evaluate(element('close') + '.focus()');
  await key('ArrowLeft', 37);
  assert.equal(await text(element('dialog-title')), 'Photo 5');
  await key('ArrowRight', 39);
  assert.equal(await text(element('dialog-title')), 'Portrait.CR2');
  await key('Escape', 27);
  await until('document.activeElement === ' + element('enlarge'), 'Focus did not return to the pane opener');
  await send('Emulation.setDeviceMetricsOverride', {
    width: 760, height: 680, deviceScaleFactor: 1, mobile: false,
  });
  await evaluate(button('Portrait.CR2') + '.focus({preventScroll: true})');
  await click(button('Portrait.CR2'));
  // Chromium rounds the scroll offset to a pixel. Check the decoded layout
  // with room for a fractional pixel at the top of the pane.
  await until(paneImage + '?.naturalHeight === 800 && ' +
    element('pane') + '.getBoundingClientRect().top >= -1 && ' +
    element('enlarge') + '.getBoundingClientRect().bottom < innerHeight',
    'The stacked pane was not brought into the narrow viewport');
  assert.equal(await evaluate('document.activeElement.id'), 'photo-enlarge');
  await send('Emulation.setDeviceMetricsOverride', {
    width: 1000, height: 680, deviceScaleFactor: 1, mobile: false,
  });
  await click(element('pane-toggle'));

  await click(button('Broken.CR2'));
  await until(element('dialog-note') + '.textContent.includes("could not decode")', 'Decode failure did not appear');
  await key('Escape', 27);
  await until('document.activeElement === ' + button('Broken.CR2'), 'Focus did not return to the row');
  await click(button('Orchard.CR2'));
  assert.match(await text(element('dialog-note')), /index lists 720 . 500 px, but the JPEG is 768 . 512 px/);
  assert.equal(await until(dialogImage + '?.naturalWidth', 'The suspect JPEG did not decode'), 768);
  await key('Escape', 27);
  await until(element('dialog') + '.open === false', 'The dialog did not close');

  await click(element('selected'));
  assert.match(await count(), /6 shown . 6 checked/);
  await click(button('Bridge.CR2'));
  await key('ArrowRight', 39);
  assert.equal(await text(element('dialog-title')), 'Broken.CR2');
  await click(element('dialog-check'));
  await key('Escape', 27);
  await until('document.activeElement === ' + button('Bridge.CR2'),
    'Focus did not return to the replacement of the still-visible opener');

  await set('search', 'Bridge', 'input');
  await click(element('pane-toggle'));
  await click(element('enlarge'));
  await click(element('dialog-check'));
  assert.equal(await evaluate(element('enlarge') + '.disabled'), true);
  await key('Escape', 27);
  await until('document.activeElement === ' + element('search'),
    'Focus did not fall back to search when the pane opener became disabled');

  await click(element('pane-toggle'));
  await click(element('selected'));
  await set('search', '', 'input');
  await click(element('check-visible'));
  await set('search', 'Creek', 'input');
  await click(element('uncheck-visible'));
  await set('search', 'Island', 'input');
  await click(element('uncheck-visible'));
  await set('search', '', 'input');
  await click(element('selected'));
  assert.match(await count(), /6 shown . 6 checked/);
  await click(button('Meadow.CR2'));
  await click(element('dialog-check'));
  assert.match(await count(), /5 shown . 5 checked/);
  assert.equal(await evaluate(button('Meadow.CR2') + ' === null'), true);
  await key('ArrowRight', 39);
  assert.equal(await text(element('dialog-title')), 'Orchard.CR2');
  await key('Escape', 27);
  await until('document.activeElement === ' + element('search'), 'Focus did not fall back to search');

  await click(lookup('#roots button'));
  await until(lookup('#roots').concat('.textContent.includes("available from this page")'), 'Recheck did not complete after review');
  assert.match(await count(), /10 shown . 5 checked/);
  assert.match(await text(element('groups')), /Meadow.CR2Original is reachable/);
  await click(lookup('#restart'));
  await scan();
  assert.match(await count(), /10 shown . 6 checked/);
  await click(lookup('#restart'));
  await scan(false);
  assert.match(await count(), /10 shown . 0 checked/);
  assert.match(await text(element('groups')), /Photos without a recoverable preview.*Meadow.CR2/s);
  assert.deepEqual(await evaluate('reviewHashes()'), sourceHashes);

  return { width: 1000, height: 680, stackedWidth: 760, maxThumbnailReads: backgroundReads, portraitFitHeight: fitH, portraitActualHeight: actualH, catalogFilesUnchanged: sourceHashes.length };
}
