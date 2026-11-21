const output = document.querySelector('[data-build-identity]');
if (output) {
  try {
    const response = await fetch(new URL('./build.json', import.meta.url), { cache: 'no-cache' });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const { app, core, commit, dirty } = await response.json();
    output.textContent = `App ${app} · Core ${core} · ${commit.slice(0, 12)}${dirty ? ' (local changes)' : ''}`;
  } catch {
    output.textContent = 'Build versions unavailable';
  }
}
