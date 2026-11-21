// Review owns the choices for one open scan. Visibility and selection are independent.
const MAX_THUMBNAIL_READS = 2;

export function mountReview(document) {
  const el = (name) => document.getElementById('photo-' + name);
  const section = el('review');
  const search = el('search');
  const groups = el('groups');
  const layout = el('layout');
  const thumbnails = el('thumbnails');
  const selectedOnly = el('selected');
  const pane = el('pane');
  const paneToggle = el('pane-toggle');
  const dialog = el('dialog');
  const loupe = el('dialog-image');
  const dialogCheck = el('dialog-check');
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

  let photos = [];
  let checked = new Set();
  let collapsed = new Set();
  let displayed = [];
  let selectedPhoto;
  let enlargedPhoto;
  let returnFocus;
  let dialogOrder = [];
  let read;
  let clearPaneImage;
  let clearLoupeImage;
  const inFlight = new Map();
  const tiles = new Map();
  const tileImages = new Map();
  const thumbnailQueue = new Set();
  let thumbnailReads = 0;
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      const tile = tileImages.get(entry.target);
      if (!tile) continue;
      if (entry.isIntersecting) {
        if (!tile.clear) thumbnailQueue.add(entry.target);
      } else {
        thumbnailQueue.delete(entry.target);
        tile.clear?.();
        tile.clear = null;
      }
    }
    pumpThumbnails();
  }, { rootMargin: '160px' });

  // The worker reads one payload at a time. Never make the pane or dialog
  // wait behind more than two thumbnail reads, and skip tiles that scrolled away.
  function pumpThumbnails() {
    while (thumbnailReads < MAX_THUMBNAIL_READS && thumbnailQueue.size) {
      const target = thumbnailQueue.values().next().value;
      thumbnailQueue.delete(target);
      const tile = tileImages.get(target);
      thumbnailReads++;
      tile.clear = showImage(tile.photo, target, null, () => {
        thumbnailReads--;
        pumpThumbnails();
      });
    }
  }

  const sections = [
    { name: 'Previews', matches: (photo) => photo.preview && photo.checked },
    { name: 'Small previews', matches: (photo) => photo.preview && !photo.checked },
    { name: 'Photos without a recoverable preview', matches: (photo) => !photo.preview },
  ];
  const noPreview = {
    'missing-payload': 'Preview file is missing',
    'no-cache-entry': 'No entry in the preview index',
    failed: 'Preview is damaged or unreadable',
    'original-present': 'Original is reachable',
  };

  search.addEventListener('input', renderRows);
  for (const control of [el('sort'), el('group'), selectedOnly]) {
    control.addEventListener('change', renderRows);
  }
  layout.addEventListener('change', setLayout);
  thumbnails.addEventListener('change', setLayout);
  paneToggle.addEventListener('change', updatePane);
  el('check-visible').addEventListener('click', () => markVisible(true));
  el('uncheck-visible').addEventListener('click', () => markVisible(false));
  el('enlarge').addEventListener('click', () => open(selectedPhoto, () => el('enlarge')));

  el('fit').addEventListener('click', () => setActual(false));
  el('actual').addEventListener('click', () => setActual(true));
  el('previous').addEventListener('click', () => step(-1));
  el('next').addEventListener('click', () => step(1));
  dialogCheck.addEventListener('change', () => mark([enlargedPhoto], dialogCheck.checked));
  dialog.addEventListener('keydown', (event) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    if (event.target === loupe && loupe.classList.contains('actual')) return;
    event.preventDefault();
    step(event.key === 'ArrowLeft' ? -1 : 1);
  });
  dialog.addEventListener('close', () => {
    if (dialog.open) return;
    clearLoupeImage?.();
    clearLoupeImage = null;
    enlargedPhoto = null;
    dialogOrder = [];
    if (!section.hidden && returnFocus) {
      const opener = returnFocus();
      const target = opener?.isConnected && !opener.disabled &&
        !opener.closest('[hidden], details:not([open])')
        ? opener
        : search;
      target.focus();
    }
    returnFocus = null;
  });

  function clearTiles() {
    observer.disconnect();
    thumbnailQueue.clear();
    for (const tile of tileImages.values()) tile.clear?.();
    tileImages.clear();
    tiles.clear();
  }

  function clear() {
    section.hidden = true;
    if (dialog.open) dialog.close();
    clearTiles();
    clearPaneImage?.();
    clearLoupeImage?.();
    clearPaneImage = null;
    clearLoupeImage = null;
    groups.replaceChildren();
    photos = [];
    displayed = [];
    checked.clear();
    collapsed.clear();
    selectedPhoto = null;
    enlargedPhoto = null;
    dialogOrder = [];
    returnFocus = null;
    read = null;
    inFlight.clear();
  }

  function show(rows, readPreview) {
    clear();
    photos = rows;
    read = readPreview;
    checked = new Set(photos.filter((photo) => photo.checked).map((photo) => photo.id));
    search.value = '';
    selectedOnly.checked = false;
    section.hidden = false;
    setLayout();
    renderRows();
  }

  function setLayout() {
    groups.classList.toggle('as-list', layout.value === 'List');
    groups.dataset.thumbnailSize = thumbnails.value;
  }

  function filteredPhotos() {
    const term = search.value.trim().toLocaleLowerCase();
    const order = el('sort').value;
    return photos.filter((photo) =>
      (!selectedOnly.checked || checked.has(photo.id)) &&
      (!term || [photo.name, folderName(photo)].some((text) => text.toLocaleLowerCase().includes(term))),
    ).sort((a, b) => {
      let precedence = 0;
      if (order === 'folder') {
        precedence = a.folder && b.folder
          ? collator.compare(a.folder, b.folder)
          : Number(!a.folder) - Number(!b.folder);
      }
      if (order === 'size') precedence = longEdge(b) - longEdge(a);
      return precedence || collator.compare(a.name, b.name);
    });
  }

  function renderRows() {
    clearTiles();
    groups.replaceChildren();
    displayed = [];
    const matches = filteredPhotos();
    for (const { name, matches: isInSection } of sections) {
      const rows = matches.filter(isInSection);
      if (!rows.length) continue;
      const group = element('section', 'review-group');
      group.append(element('h3', '', name));
      if (el('group').checked) {
        const folders = Map.groupBy(rows, (photo) => photo.folder);
        for (const [folder, photos] of folders) {
          const key = JSON.stringify([name, folder]);
          const details = element('details', 'review-folder');
          details.open = !collapsed.has(key);
          details.addEventListener('toggle', () => {
            if (!details.isConnected) return;
            if (details.open) collapsed.delete(key);
            else collapsed.add(key);
            updateCounts();
            updatePane();
            updateNavigation();
          });
          details.append(element('summary', '', folderName(photos[0]) + ' (' + photos.length + ')'));
          details.append(tilesFor(photos, details));
          group.append(details);
        }
      } else {
        group.append(tilesFor(rows));
      }
      groups.append(group);
    }
    if (!matches.length) {
      groups.append(element('p', '', photos.length
        ? 'No photos match these filters.'
        : 'This catalog does not contain any photos.'));
    }
    updateCounts();
    updatePane();
    updateNavigation();
  }

  function tilesFor(rows, folder) {
    const list = element('ul', 'review-grid');
    for (const photo of rows) {
      displayed.push({ photo, folder });
      const tile = element('li', 'review-tile');
      if (!photo.preview) {
        tile.append(element('span', 'review-name', photo.name));
        tile.append(element('small', 'review-detail', noPreview[photo.status] ?? 'No preview available'));
        list.append(tile);
        continue;
      }

      const button = element('button', 'review-open');
      button.type = 'button';
      button.setAttribute('aria-label', 'Preview ' + photo.name);
      button.addEventListener('click', () => {
        selectedPhoto = photo;
        if (paneToggle.checked) {
          updatePane();
          if (getComputedStyle(groups.parentElement).display !== 'grid') {
            pane.scrollIntoView({ block: 'start' });
            el('enlarge').focus({ preventScroll: true });
          }
        } else {
          open(photo, () => tiles.get(photo.id)?.button);
        }
      });
      const image = element('div', 'review-image thumb');
      button.append(image, element('span', 'review-name', photo.name));
      observer.observe(image);
      tileImages.set(image, { photo });

      const label = element('label', 'review-check');
      const checkbox = element('input');
      checkbox.type = 'checkbox';
      checkbox.checked = checked.has(photo.id);
      checkbox.setAttribute('aria-label', 'Include ' + photo.name);
      checkbox.addEventListener('change', () => {
        mark([photo], checkbox.checked);
        if (!checkbox.isConnected) search.focus();
      });
      label.append(checkbox, document.createTextNode(' Include'));
      tile.append(button, element('small', 'review-detail', dimensions(photo)), label);
      list.append(tile);
      tiles.set(photo.id, { button, checkbox });
    }
    return list;
  }

  function visible() {
    return displayed.filter(({ folder }) => !folder || folder.open).map(({ photo }) => photo);
  }

  function updateCounts() {
    const shown = visible();
    el('count').textContent = shown.length + ' shown · ' + checked.size + ' checked';
    el('check-visible').disabled = !shown.some((photo) => photo.preview && !checked.has(photo.id));
    el('uncheck-visible').disabled = !shown.some((photo) => checked.has(photo.id));
  }

  function markVisible(include) {
    mark(visible().filter((photo) => photo.preview), include);
    if (document.activeElement === document.body) {
      const next = include ? el('uncheck-visible') : el('check-visible');
      (next.disabled ? search : next).focus();
    }
  }

  function mark(rows, include) {
    for (const photo of rows) {
      if (include) checked.add(photo.id);
      else checked.delete(photo.id);
      const tile = tiles.get(photo.id);
      if (tile) tile.checkbox.checked = include;
    }
    if (enlargedPhoto) dialogCheck.checked = checked.has(enlargedPhoto.id);
    if (selectedOnly.checked) renderRows();
    else updateCounts();
  }

  function updatePane() {
    pane.hidden = !paneToggle.checked;
    groups.parentElement.classList.toggle('with-pane', !pane.hidden);
    clearPaneImage?.();
    clearPaneImage = null;
    for (const { button } of tiles.values()) button.removeAttribute('aria-current');
    if (pane.hidden) return;

    const previews = visible().filter((photo) => photo.preview);
    if (!previews.includes(selectedPhoto)) selectedPhoto = previews[0];
    const photo = selectedPhoto;
    el('pane-name').textContent = photo?.name ?? 'No visible preview';
    el('pane-size').textContent = photo ? dimensions(photo) : '';
    el('enlarge').disabled = !photo;
    if (!photo) {
      el('pane-note').hidden = true;
      return;
    }
    tiles.get(photo.id)?.button.setAttribute('aria-current', 'true');
    clearPaneImage = showImage(photo, el('pane-image'), el('pane-note'));
  }

  function open(photo, resolveOpener) {
    if (!photo) return;
    returnFocus = resolveOpener;
    dialogOrder = visible().filter((candidate) => candidate.preview);
    setActual(false);
    showLoupe(photo);
    dialog.showModal();
  }

  function showLoupe(photo) {
    enlargedPhoto = photo;
    el('dialog-title').textContent = photo.name;
    el('dialog-size').textContent = dimensions(photo);
    dialogCheck.checked = checked.has(photo.id);
    clearLoupeImage?.();
    loupe.scrollTop = 0;
    loupe.scrollLeft = 0;
    clearLoupeImage = showImage(photo, loupe, el('dialog-note'));
    updateNavigation();
  }

  function setActual(actual) {
    loupe.classList.toggle('actual', actual);
    el('fit').setAttribute('aria-pressed', String(!actual));
    el('actual').setAttribute('aria-pressed', String(actual));
  }

  function neighbor(offset) {
    const position = dialogOrder.indexOf(enlargedPhoto);
    for (let i = position + offset; i >= 0 && i < dialogOrder.length; i += offset) {
      const photo = dialogOrder[i];
      if (!selectedOnly.checked || checked.has(photo.id)) return photo;
    }
  }

  function step(offset) {
    const photo = neighbor(offset);
    if (photo) showLoupe(photo);
  }

  function updateNavigation() {
    if (!enlargedPhoto) return;
    el('previous').disabled = !neighbor(-1);
    el('next').disabled = !neighbor(1);
  }

  function showImage(photo, destination, note = null, onRead = null) {
    const notice = element('span', 'review-placeholder', 'Loading preview…');
    destination.replaceChildren(notice);
    let active = true;
    let url;
    const warning = photo.status === 'suspect' && photo.declared
      ? 'The preview index lists ' + sizeText(photo.declared) +
        ', but the JPEG is ' + sizeText(photo.size) + '.'
      : '';
    if (note) {
      note.textContent = warning;
      note.hidden = !warning;
    }
    if (!inFlight.has(photo.id)) {
      const pending = read(photo.id).finally(() => {
        if (inFlight.get(photo.id) === pending) inFlight.delete(photo.id);
      });
      inFlight.set(photo.id, pending);
    }
    const request = inFlight.get(photo.id);
    if (onRead) void request.then(onRead, onRead);
    void (async () => {
      let blob;
      try {
        blob = await request;
      } catch (problem) {
        fail(problem?.kind === 'changed-size'
          ? 'The preview was ' + sizeText(problem.expected) + ' at scan time. It is now ' +
            sizeText(problem.actual) + '. Scan again to review the current file.'
          : 'Could not read this preview. The cache may have changed since the scan.');
        return;
      }
      if (!active) return;
      const image = element('img');
      image.alt = photo.name;
      url = URL.createObjectURL(blob);
      image.src = url;
      try {
        await image.decode();
      } catch {
        fail('This browser could not decode the preview JPEG.');
        return;
      }
      if (!active) return;
      const expected = photo.rotation % 180 === 90 ? [...photo.size].reverse() : photo.size;
      const actual = [image.naturalWidth, image.naturalHeight];
      if (actual[0] !== expected[0] || actual[1] !== expected[1]) {
        const difference = 'The browser decoded ' + sizeText(actual) +
          ' but expected ' + sizeText(expected) + '.';
        if (note) setNotice(difference);
        else {
          notice.textContent = difference;
          destination.replaceChildren(image, notice);
          return;
        }
      }
      destination.replaceChildren(image);
    })();

    function fail(message) {
      if (!active) return;
      notice.textContent = note ? 'Preview unavailable' : message;
      if (note) setNotice(message);
      if (url) URL.revokeObjectURL(url);
      url = null;
    }

    function setNotice(message) {
      note.textContent = [warning, message].filter(Boolean).join(' ');
      note.hidden = false;
    }

    return () => {
      active = false;
      destination.replaceChildren();
      if (url) URL.revokeObjectURL(url);
    };
  }

  function element(tag, className = '', text = '') {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text) node.textContent = text;
    return node;
  }

  return { show, clear };
}

function folderName(photo) {
  return photo.folder ?? 'Unknown folder';
}

function longEdge(photo) {
  return photo.preview ? Math.max(...photo.size) : 0;
}

function sizeText([width, height]) {
  return width + ' × ' + height + ' px';
}

function dimensions(photo) {
  return 'JPEG ' + sizeText(photo.size) + (photo.rotation ? ' · ' + photo.rotation + '° rotation' : '');
}
