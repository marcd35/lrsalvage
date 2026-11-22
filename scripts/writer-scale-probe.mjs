// Runs in the recovery page via DevTools. All handles and writes remain native.
export function installWriterProbe(destination, holdForStop, total) {
  const el = (id) => document.getElementById(id);
  const start = el('export-start');
  const stop = el('export-stop');
  const status = el('export-status');
  const directories = new WeakMap();
  const files = new WeakMap();
  const opens = [];
  const commits = [];
  const held = new Map();
  const picker = window.showDirectoryPicker;
  let picked = false;
  let started = null;
  let progress = null;
  let stopped = null;
  let holding = false;
  let positioned = false;
  let longestGap = null;

  window.showDirectoryPicker = async function (options) {
    const handle = await picker.call(this, options);
    if (options.mode === 'readwrite') {
      picked = handle.name === destination;
      if (picked) directories.set(handle, '');
    }
    return handle;
  };

  const directory = FileSystemDirectoryHandle.prototype;
  const getDirectory = directory.getDirectoryHandle;
  directory.getDirectoryHandle = async function (name, options) {
    const handle = await getDirectory.call(this, name, options);
    if (directories.has(this)) directories.set(handle, `${directories.get(this)}${name}/`);
    return handle;
  };
  const getFile = directory.getFileHandle;
  directory.getFileHandle = async function (name, options) {
    const handle = await getFile.call(this, name, options);
    if (directories.has(this) && /\.jpe?g$/i.test(name)) {
      files.set(handle, directories.get(this) + name);
    }
    return handle;
  };
  const file = FileSystemFileHandle.prototype;
  const createWritable = file.createWritable;
  file.createWritable = async function (...args) {
    const path = files.get(this);
    if (path !== undefined) opens.push(path);
    const stream = await createWritable.apply(this, args);
    if (path !== undefined) {
      const close = stream.close.bind(stream);
      stream.close = async () => {
        if (holding) await new Promise((release) => held.set(path, release));
        await close();
        const time = performance.now();
        if (commits.length) longestGap = Math.max(longestGap ?? 0, time - commits.at(-1)[1]);
        commits.push([path, time]);
      };
    }
    return stream;
  };

  function observe() {
    if (progress === null && started !== null) {
      const saved = Number(status.textContent.match(/^(\d+) saved of /)?.[1]);
      if (!positioned && Number.isFinite(saved)) {
        status.scrollIntoView({ block: 'center' });
        positioned = true;
      }
      if (saved > 0 && saved < total && !stop.hidden && document.visibilityState === 'visible' &&
          document.hasFocus() &&
          status.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true })) {
        const rect = status.getBoundingClientRect();
        if (rect.bottom > 0 && rect.top < innerHeight) progress = saved;
      }
    }
    if (holdForStop && stopped === null && progress !== null) {
      holding = true;
      if (held.size === 8 && !stop.disabled && !stop.hidden) {
        stopped = { opened: opens.length, committed: commits.length, inFlight: [...held.keys()] };
        stop.click();
        holding = false;
        for (const release of held.values()) release();
        held.clear();
      }
    }
    if (progress === null || (holdForStop && stopped === null)) requestAnimationFrame(observe);
  }

  return {
    ready: () => picked && !start.disabled && el('export-layout').value === 'mirrored' &&
      el('export-total').textContent.startsWith(`${total} JPEGs `),
    start() {
      started = performance.now();
      start.click();
      requestAnimationFrame(observe);
    },
    continue: () => start.click(),
    state: () => ({
      settled: !start.disabled && started !== null && stop.hidden,
      visible_saved_count: progress,
      writer_opens: opens.length,
      jpeg_closes: commits.length,
      elapsed_ms: commits.length ? commits.at(-1)[1] - started : null,
      longest_gap_ms: longestGap,
    }),
    snapshot: () => ({
      progress, stopped, opens, commits, status: status.textContent,
      headline: el('receipt-headline').textContent,
      detail: el('receipt-detail').textContent,
      reconciled: el('receipt-reconciled').textContent,
      destination: el('receipt-destination').textContent,
      failures: el('receipt-failures').childElementCount,
    }),
  };
}
