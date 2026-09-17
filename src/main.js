// FullFrame main control window.
document.getElementById('btnRegion').onclick = () => invoke('begin_region_capture');
document.getElementById('btnFull').onclick = () => invoke('capture_fullscreen');
document.getElementById('btnLong').onclick = () => {
  toast('Install the companion extension from the extension/ folder — see notes below.');
};

// Delayed capture: the backend counts down via `delay_tick` events, hides
// this window right before the shot, then captures.
document.getElementById('btnDelay').onclick = () => {
  const kind = document.getElementById('delayKind').value;
  const seconds = parseInt(document.getElementById('delaySecs').value, 10);
  invoke('capture_delayed', { kind, seconds });
};
listen('delay_tick', (e) => {
  const cd = document.getElementById('countdown');
  const n = e.payload.remaining;
  if (n > 0) {
    cd.style.display = 'flex';
    document.getElementById('countNum').textContent = n;
    document.getElementById('countKind').textContent =
      `Capturing ${e.payload.kind} — get your menus ready`;
  } else {
    cd.style.display = 'none';
  }
});
listen('delay_done', () => {
  document.getElementById('countdown').style.display = 'none';
});
listen('delay_error', (e) => {
  document.getElementById('countdown').style.display = 'none';
  toast('Delayed capture failed: ' + (e.payload.error || 'unknown error'));
});

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
