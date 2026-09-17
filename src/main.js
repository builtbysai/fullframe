// FullFrame main control window.
document.getElementById('btnRegion').onclick = () => invoke('begin_region_capture');
document.getElementById('btnFull').onclick = () => invoke('capture_fullscreen');
document.getElementById('btnLong').onclick = () => {
  toast('Install the companion extension from the extension/ folder — see notes below.');
};

async function loadWindows() {
  const list = document.getElementById('winList');
  try {
    const wins = await invoke('list_windows');
    list.innerHTML = '';
    if (!wins.length) {
      list.innerHTML = '<p class="sub">No windows found.</p>';
      return;
    }
    wins.slice(0, 40).forEach((w) => {
      const b = document.createElement('button');
      b.className = 'btn';
      const label = document.createElement('span');
      label.className = 'meta';
      const title = document.createElement('b');
      title.textContent = w.title.length > 48 ? w.title.slice(0, 48) + '…' : w.title;
      const sub = document.createElement('small');
      sub.textContent = `${w.app_name} · ${w.width}×${w.height}${w.is_minimized ? ' · minimized' : ''}`;
      label.appendChild(title);
      label.appendChild(sub);
      b.appendChild(label);
      b.onclick = () => invoke('capture_window', { id: w.id });
      list.appendChild(b);
    });
  } catch (err) {
    list.innerHTML = '<p class="sub">Could not list windows.</p>';
  }
}
loadWindows();
setInterval(loadWindows, 10000);
