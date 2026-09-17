// Region-select overlay. The backend captured this monitor already and sends
// the screenshot as a data URL; we show it dimmed, track the mouse with a
// crosshair + magnifier + live coordinates, and report the logical-pixel
// selection back. The backend converts to physical pixels using the monitor's
// scale factor and crops from the stored capture (no second screenshot).

let monitor = null;   // {id, x, y, w, h, scale}
let dragging = false;
let startX = 0, startY = 0;

const bg = document.getElementById('bg');
const sel = document.getElementById('sel');
const crossX = document.getElementById('crossX');
const crossY = document.getElementById('crossY');
const coords = document.getElementById('coords');
const mag = document.getElementById('mag');
const magCtx = mag.getContext('2d');
const hint = document.getElementById('hint');
magCtx.imageSmoothingEnabled = false;

listen('overlay-preview', (e) => {
  monitor = e.payload.monitor;
  bg.src = e.payload.data_url;
});

function show(el, x, y) {
  el.style.display = 'block';
}

function updateMagnifier(mx, my) {
  if (!bg.complete || !bg.naturalWidth) return;
  const zoom = 6, size = 180;
  const srcW = size / zoom, srcH = size / zoom;
  // Map CSS px -> screenshot px (the img fills the window exactly).
  const sx = (mx / window.innerWidth) * bg.naturalWidth - srcW / 2;
  const sy = (my / window.innerHeight) * bg.naturalHeight - srcH / 2;
  magCtx.clearRect(0, 0, size, size);
  magCtx.drawImage(bg, sx, sy, srcW, srcH, 0, 0, size, size);
  // Center cross.
  magCtx.strokeStyle = '#22d3ee';
  magCtx.lineWidth = 1;
  magCtx.beginPath();
  magCtx.moveTo(size / 2, 0); magCtx.lineTo(size / 2, size);
  magCtx.moveTo(0, size / 2); magCtx.lineTo(size, size / 2);
  magCtx.stroke();
  // Place magnifier near cursor, flipped at edges.
  const off = 24;
  let lx = mx + off, ly = my + off;
  if (lx + size > window.innerWidth) lx = mx - size - off;
  if (ly + size > window.innerHeight) ly = my - size - off;
  mag.style.left = lx + 'px';
  mag.style.top = ly + 'px';
  mag.style.display = 'block';
}

document.addEventListener('mousemove', (e) => {
  const mx = e.clientX, my = e.clientY;
  crossX.style.display = 'block';
  crossY.style.display = 'block';
  crossX.style.top = my + 'px';
  crossY.style.left = mx + 'px';
  updateMagnifier(mx, my);

  if (dragging) {
    const x = Math.min(startX, mx), y = Math.min(startY, my);
    const w = Math.abs(mx - startX), h = Math.abs(my - startY);
    Object.assign(sel.style, { display: 'block', left: x + 'px', top: y + 'px', width: w + 'px', height: h + 'px' });
    coords.style.display = 'block';
    coords.textContent = `${Math.round(w)} × ${Math.round(h)}`;
    coords.style.left = (x + w + 12) + 'px';
    coords.style.top = (y - 8) + 'px';
  } else {
    coords.style.display = 'block';
    coords.textContent = `${Math.round(mx)}, ${Math.round(my)}`;
    coords.style.left = (mx + 16) + 'px';
    coords.style.top = (my + 16) + 'px';
  }
});

document.addEventListener('mousedown', (e) => {
  if (e.button !== 0) return;
  dragging = true;
  startX = e.clientX; startY = e.clientY;
  hint.style.display = 'none';
});

document.addEventListener('mouseup', async (e) => {
  if (!dragging || e.button !== 0) return;
  dragging = false;
  const x = Math.min(startX, e.clientX), y = Math.min(startY, e.clientY);
  const w = Math.abs(e.clientX - startX), h = Math.abs(e.clientY - startY);
  if (w < 5 || h < 5 || !monitor) {
    // Tiny drag = treat as a click: cancel.
    await invoke('cancel_capture');
    return;
  }
  await invoke('finish_region_capture', { monitor_id: monitor.id, x, y, w, h });
});

document.addEventListener('dblclick', async () => {
  if (!monitor) return;
  // Double-click = capture this whole monitor.
  await invoke('finish_region_capture', { monitor_id: monitor.id, x: 0, y: 0, w: monitor.w, h: monitor.h });
});

document.addEventListener('keydown', async (e) => {
  if (e.key === 'Escape') await invoke('cancel_capture');
});
