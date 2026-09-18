// FullFrame annotation editor.
// Canvas model: `base` (baked offscreen canvas) + `shapes` (live vector
// overlays). Destructive ops (blur/pixelate/crop) composite shapes into base
// first. Undo snapshots both.

const cv = document.getElementById('cv');
const ctx = cv.getContext('2d');
const wrap = document.getElementById('canvasWrap');
const stage = document.getElementById('stage');
const emptyMsg = document.getElementById('empty');
const dimsEl = document.getElementById('dims');
const textInput = document.getElementById('textInput');

const base = document.createElement('canvas');
const bctx = base.getContext('2d');

let shapes = [];        // live vector shapes
let tool = 'select';
let color = '#ef4444';
let lineWidth = 3;
let zoom = 1;
let undoStack = [], redoStack = [];
let selected = -1;      // index into shapes
let dragState = null;   // {mode, ...} while mouse is down
let stepCounter = 1;

const COLORS = ['#ef4444', '#f59e0b', '#22c55e', '#3b82f6', '#a855f7', '#ec4899', '#ffffff', '#111827'];
const colorsEl = document.getElementById('colors');
COLORS.forEach((c, i) => {
  const b = document.createElement('button');
  b.className = 'swatch' + (i === 0 ? ' active' : '');
  b.style.background = c;
  b.title = c;
  b.onclick = () => {
    color = c;
    colorsEl.querySelectorAll('.swatch').forEach(s => s.classList.remove('active'));
    b.classList.add('active');
  };
  colorsEl.appendChild(b);
});

// ------------------------------------------------------------ history ---

function snapshot() {
  return { img: base.toDataURL('image/png'), shapes: JSON.stringify(shapes), step: stepCounter };
}
function restore(s) {
  const img = new Image();
  img.onload = () => {
    base.width = img.naturalWidth; base.height = img.naturalHeight;
    bctx.drawImage(img, 0, 0);
    shapes = JSON.parse(s.shapes);
    stepCounter = s.step;
    selected = -1;
    fitCanvas();
    render();
  };
  img.src = s.img;
}
function pushUndo() {
  undoStack.push(snapshot());
  if (undoStack.length > 40) undoStack.shift();
  redoStack = [];
  updateHistoryButtons();
}
function updateHistoryButtons() {
  document.getElementById('undo').disabled = !undoStack.length;
  document.getElementById('redo').disabled = !redoStack.length;
}
function doUndo() {
  if (!undoStack.length) return;
  redoStack.push(snapshot());
  restore(undoStack.pop());
  updateHistoryButtons();
}
function doRedo() {
  if (!redoStack.length) return;
  undoStack.push(snapshot());
  restore(redoStack.pop());
  updateHistoryButtons();
}

// ------------------------------------------------------------- canvas ---

function fitCanvas() {
  cv.width = base.width; cv.height = base.height;
  dimsEl.textContent = `${base.width} × ${base.height}px`;
  applyZoom();
}
function applyZoom() {
  wrap.style.transform = `scale(${zoom})`;
  // Keep it roughly centered: pad the stage by the scaled overflow.
  const w = base.width * zoom, h = base.height * zoom;
  wrap.style.marginRight = Math.max(0, (stage.clientWidth - w) / 2) + 'px';
}

function drawArrow(c, x0, y0, x1, y1, color, width) {
  const head = Math.max(10, width * 4);
  const ang = Math.atan2(y1 - y0, x1 - x0);
  c.strokeStyle = color; c.fillStyle = color; c.lineWidth = width;
  c.lineCap = 'round';
  c.beginPath(); c.moveTo(x0, y0); c.lineTo(x1, y1); c.stroke();
  c.beginPath();
  c.moveTo(x1, y1);
  c.lineTo(x1 - head * Math.cos(ang - 0.42), y1 - head * Math.sin(ang - 0.42));
  c.lineTo(x1 - head * Math.cos(ang + 0.42), y1 - head * Math.sin(ang + 0.42));
  c.closePath(); c.fill();
}

function drawShape(c, s) {
  c.save();
  switch (s.type) {
    case 'rect':
      c.strokeStyle = s.color; c.lineWidth = s.width;
      c.strokeRect(Math.min(s.x0, s.x1), Math.min(s.y0, s.y1), Math.abs(s.x1 - s.x0), Math.abs(s.y1 - s.y0));
      break;
    case 'ellipse':
      c.strokeStyle = s.color; c.lineWidth = s.width;
      c.beginPath();
      c.ellipse((s.x0 + s.x1) / 2, (s.y0 + s.y1) / 2, Math.abs(s.x1 - s.x0) / 2, Math.abs(s.y1 - s.y0) / 2, 0, 0, Math.PI * 2);
      c.stroke();
      break;
    case 'arrow': drawArrow(c, s.x0, s.y0, s.x1, s.y1, s.color, s.width); break;
    case 'line':
      c.strokeStyle = s.color; c.lineWidth = s.width; c.lineCap = 'round';
      c.beginPath(); c.moveTo(s.x0, s.y0); c.lineTo(s.x1, s.y1); c.stroke();
      break;
    case 'pen':
    case 'highlight':
      c.strokeStyle = s.color; c.lineWidth = s.width; c.lineCap = 'round'; c.lineJoin = 'round';
      if (s.type === 'highlight') c.globalAlpha = 0.45;
      c.beginPath();
      s.points.forEach((p, i) => i ? c.lineTo(p[0], p[1]) : c.moveTo(p[0], p[1]));
      c.stroke();
      break;
    case 'text':
      c.fillStyle = s.color;
      c.font = `600 ${s.size}px system-ui, sans-serif`;
      c.textBaseline = 'top';
      // Subtle dark outline for readability on any background.
      c.strokeStyle = 'rgba(0,0,0,0.55)'; c.lineWidth = 3;
      c.strokeText(s.text, s.x, s.y);
      c.fillText(s.text, s.x, s.y);
      break;
    case 'step': {
      const r = 14;
      c.fillStyle = s.color;
      c.beginPath(); c.arc(s.x, s.y, r, 0, Math.PI * 2); c.fill();
      c.fillStyle = '#fff'; c.font = '700 15px system-ui, sans-serif';
      c.textAlign = 'center'; c.textBaseline = 'middle';
      c.fillText(String(s.n), s.x, s.y + 1);
      break;
    }
  }
  c.restore();
}

function render() {
  ctx.clearRect(0, 0, cv.width, cv.height);
  ctx.drawImage(base, 0, 0);
  shapes.forEach((s, i) => {
    drawShape(ctx, s);
    if (i === selected) {
      const b = shapeBounds(s);
      ctx.save();
      ctx.strokeStyle = '#22d3ee'; ctx.lineWidth = 1.5; ctx.setLineDash([6, 4]);
      ctx.strokeRect(b.x - 6, b.y - 6, b.w + 12, b.h + 12);
      ctx.restore();
    }
  });
}

function shapeBounds(s) {
  if (s.type === 'pen' || s.type === 'highlight') {
    const xs = s.points.map(p => p[0]), ys = s.points.map(p => p[1]);
    const x0 = Math.min(...xs), y0 = Math.min(...ys);
    return { x: x0, y: y0, w: Math.max(...xs) - x0, h: Math.max(...ys) - y0 };
  }
  if (s.type === 'text') {
    ctx.save();
    ctx.font = `600 ${s.size}px system-ui, sans-serif`;
    const w = ctx.measureText(s.text).width;
    ctx.restore();
    return { x: s.x, y: s.y, w, h: s.size };
  }
  if (s.type === 'step') return { x: s.x - 14, y: s.y - 14, w: 28, h: 28 };
  return { x: Math.min(s.x0, s.x1), y: Math.min(s.y0, s.y1), w: Math.abs(s.x1 - s.x0), h: Math.abs(s.y1 - s.y0) };
}

function hitShape(x, y) {
  for (let i = shapes.length - 1; i >= 0; i--) {
    const b = shapeBounds(shapes[i]);
    if (x >= b.x - 8 && x <= b.x + b.w + 8 && y >= b.y - 8 && y <= b.y + b.h + 8) return i;
  }
  return -1;
}

/// Composite all live shapes into the base image and clear them.
function bake() {
  // The display canvas is re-rendered clean (no drag preview) before every
  // bake call site, so copying it here is safe.
  bctx.drawImage(cv, 0, 0);
  shapes = [];
  selected = -1;
}

// -------------------------------------------------------------- events ---

function toImageCoords(e) {
  const r = cv.getBoundingClientRect();
  return [(e.clientX - r.left) / zoom, (e.clientY - r.top) / zoom];
}

cv.addEventListener('mousedown', (e) => {
  if (!base.width) return;
  const [x, y] = toImageCoords(e);
  if (tool === 'select') {
    const i = hitShape(x, y);
    selected = i;
    if (i >= 0) {
      const s = shapes[i];
      const b = shapeBounds(s);
      dragState = { mode: 'move', idx: i, dx: x - b.x, dy: y - b.y, moved: false };
      pushUndo();
    }
    render();
    return;
  }
  if (tool === 'text') {
    // Prevent the browser's default mousedown focus change: it would blur
    // the textarea we are about to focus, instantly committing (and hiding)
    // it before the user can type.
    e.preventDefault();
    startTextInput(x, y);
    return;
  }
  if (tool === 'step') {
    pushUndo();
    shapes.push({ type: 'step', x, y, n: stepCounter++, color });
    render();
    return;
  }
  if (tool === 'pen' || tool === 'highlight') {
    pushUndo();
    const w = tool === 'highlight' ? lineWidth * 3 : lineWidth;
    const c = tool === 'highlight' ? '#ffff00' : color;
    shapes.push({ type: tool, points: [[x, y]], color: c, width: w });
    dragState = { mode: 'pen' };
    render();
    return;
  }
  // rect / ellipse / arrow / line / blur / pixelate / crop — record the drag,
  // commit on mouseup.
  dragState = { mode: 'shape', kind: tool, x0: x, y0: y, x1: x, y1: y };
});

cv.addEventListener('mousemove', (e) => {
  if (!dragState) return;
  const [x, y] = toImageCoords(e);
  if (dragState.mode === 'move') {
    const s = shapes[dragState.idx];
    const b = shapeBounds(s);
    const nx = x - dragState.dx, ny = y - dragState.dy;
    moveShape(s, nx - b.x, ny - b.y);
    dragState.moved = true;
    render();
  } else if (dragState.mode === 'pen') {
    shapes[shapes.length - 1].points.push([x, y]);
    render();
  } else if (dragState.mode === 'shape') {
    dragState.x1 = x; dragState.y1 = y;
    render();
    drawPreview(dragState);
  }
});

cv.addEventListener('mouseup', (e) => {
  if (!dragState) return;
  const [x, y] = toImageCoords(e);
  const d = dragState;
  dragState = null;
  if (d.mode === 'move') {
    if (!d.moved) { undoStack.pop(); updateHistoryButtons(); } // click w/o drag = no-op
    render();
    return;
  }
  if (d.mode === 'pen') { render(); return; }
  // shape / blur / pixelate / crop
  const x0 = Math.min(d.x0, x), y0 = Math.min(d.y0, y);
  const w = Math.abs(x - d.x0), h = Math.abs(y - d.y0);
  if (w < 4 || h < 4) { render(); return; }
  pushUndo(); // snapshot the true pre-op state (shapes still live, base unbaked)
  if (d.kind === 'blur' || d.kind === 'pixelate' || d.kind === 'crop') {
    render(); // wipe the drag preview (marching ants) so bake() can't burn it in
    bake();
    if (d.kind === 'blur') applyBlurBaked(x0, y0, w, h);
    else if (d.kind === 'pixelate') applyPixelateBaked(x0, y0, w, h);
    else applyCropBaked(x0, y0, w, h);
    render();
    return;
  }
  shapes.push({ type: d.kind, x0: d.x0, y0: d.y0, x1: x, y1: y, color, width: lineWidth });
  render();
});

// Live preview of the in-progress shape (drawn on top, not committed).
function drawPreview(d) {
  const s = { type: d.kind, x0: d.x0, y0: d.y0, x1: d.x1, y1: d.y1, color, width: lineWidth };
  if (['blur', 'pixelate', 'crop'].includes(d.kind)) {
    ctx.save();
    ctx.strokeStyle = '#22d3ee'; ctx.lineWidth = 1.5; ctx.setLineDash([6, 4]);
    const x = Math.min(d.x0, d.x1), y = Math.min(d.y0, d.y1);
    ctx.strokeRect(x, y, Math.abs(d.x1 - d.x0), Math.abs(d.y1 - d.y0));
    ctx.restore();
    return;
  }
  drawShape(ctx, s);
}

function moveShape(s, dx, dy) {
  if (s.x0 !== undefined) { s.x0 += dx; s.x1 += dx; s.y0 += dy; s.y1 += dy; }
  if (s.x !== undefined) { s.x += dx; s.y += dy; }
  if (s.points) s.points = s.points.map(p => [p[0] + dx, p[1] + dy]);
}

// Baked variants (operate directly on base; caller already pushed undo + baked).
function applyBlurBaked(x, y, w, h) {
  const tmp = document.createElement('canvas');
  const f = 0.06;
  tmp.width = Math.max(1, Math.round(w * f)); tmp.height = Math.max(1, Math.round(h * f));
  tmp.getContext('2d').drawImage(base, x, y, w, h, 0, 0, tmp.width, tmp.height);
  bctx.drawImage(tmp, 0, 0, tmp.width, tmp.height, x, y, w, h);
}
function applyPixelateBaked(x, y, w, h) {
  const px = 14;
  const tmp = document.createElement('canvas');
  tmp.width = Math.max(1, Math.round(w / px)); tmp.height = Math.max(1, Math.round(h / px));
  const t = tmp.getContext('2d');
  t.imageSmoothingEnabled = false;
  t.drawImage(base, x, y, w, h, 0, 0, tmp.width, tmp.height);
  bctx.imageSmoothingEnabled = false;
  bctx.drawImage(tmp, 0, 0, tmp.width, tmp.height, x, y, w, h);
  bctx.imageSmoothingEnabled = true;
}
function applyCropBaked(x, y, w, h) {
  const nx = Math.round(Math.max(0, x)), ny = Math.round(Math.max(0, y));
  const nw = Math.round(Math.min(w, base.width - nx)), nh = Math.round(Math.min(h, base.height - ny));
  if (nw < 2 || nh < 2) return;
  const tmp = document.createElement('canvas');
  tmp.width = nw; tmp.height = nh;
  tmp.getContext('2d').drawImage(base, nx, ny, nw, nh, 0, 0, nw, nh);
  base.width = nw; base.height = nh;
  bctx.drawImage(tmp, 0, 0);
  fitCanvas();
}

let textPos = null; // image coords where the open text box will commit
function startTextInput(x, y) {
  commitText(); // commit any pending text at its own position first
  textPos = [x, y];
  const r = cv.getBoundingClientRect();
  textInput.style.display = 'block';
  textInput.style.left = (r.left - wrap.getBoundingClientRect().left + x * zoom) + 'px';
  textInput.style.top = (r.top - wrap.getBoundingClientRect().top + y * zoom) + 'px';
  textInput.style.fontSize = (16 * zoom) + 'px';
  textInput.value = '';
  textInput.focus();
  textInput.onkeydown = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); commitText(); }
    if (e.key === 'Escape') { textInput.value = ''; commitText(); }
    e.stopPropagation();
  };
  textInput.onblur = () => commitText();
}
// Idempotent by state: the value is consumed on the first call, so the
// inevitable second call (Enter keydown followed by the async blur, or
// vice versa) finds an empty box and does nothing. No timing guards.
function commitText() {
  const v = textInput.value.trim();
  textInput.value = '';
  textInput.style.display = 'none';
  if (v && textPos) {
    pushUndo();
    shapes.push({ type: 'text', x: textPos[0], y: textPos[1], text: v, color, size: 22 });
    render();
  }
  textPos = null;
}

// ------------------------------------------------------------------ UI ---

document.querySelectorAll('#toolbar [data-tool]').forEach(b => {
  b.onclick = () => {
    tool = b.dataset.tool;
    selected = -1;
    document.querySelectorAll('#toolbar [data-tool]').forEach(x => x.classList.remove('active'));
    b.classList.add('active');
    render();
  };
});
document.querySelector('#toolbar [data-tool="select"]').classList.add('active');
document.getElementById('width').onchange = (e) => lineWidth = +e.target.value;
document.getElementById('zoom').onchange = (e) => { zoom = +e.target.value; applyZoom(); };

document.getElementById('undo').onclick = doUndo;
document.getElementById('redo').onclick = doRedo;

document.addEventListener('keydown', (e) => {
  if (e.target === textInput) return;
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === 'z' && !e.shiftKey) { e.preventDefault(); doUndo(); }
  else if (mod && (e.key.toLowerCase() === 'y' || (e.key.toLowerCase() === 'z' && e.shiftKey))) { e.preventDefault(); doRedo(); }
  else if (mod && e.key.toLowerCase() === 'c') { e.preventDefault(); copyImage(); }
  else if (mod && e.key.toLowerCase() === 's') { e.preventDefault(); saveImage(); }
  else if (e.key === 'Delete' && selected >= 0) { pushUndo(); shapes.splice(selected, 1); selected = -1; render(); }
  else if (e.key === 'Escape') { selected = -1; render(); }
  else {
    const byKey = { v: 'select', r: 'rect', e: 'ellipse', a: 'arrow', l: 'line', p: 'pen', h: 'highlight', t: 'text', n: 'step', b: 'blur', x: 'pixelate', c: 'crop' };
    const t = byKey[e.key.toLowerCase()];
    if (t && !mod) document.querySelector(`#toolbar [data-tool="${t}"]`).click();
  }
});

function compositedDataURL() {
  pushUndo(); // so Ctrl+Z after copy/save restores the live shapes
  render(); // ensure no drag preview is on the canvas before baking
  bake();
  return base.toDataURL('image/png');
}

async function copyImage() {
  if (!base.width) return;
  await invoke('copy_data_url', { dataUrl: compositedDataURL() });
  render();
  toast('Copied to clipboard');
}

async function saveImage() {
  if (!base.width) return;
  const now = new Date();
  const stamp = now.toISOString().slice(0, 19).replace(/[-:T]/g, '');
  const suggested = `fullframe-${stamp}.png`;
  const path = await invoke('plugin:dialog|save', {
    defaultPath: suggested,
    filters: [{ name: 'PNG image', extensions: ['png'] }, { name: 'JPEG image', extensions: ['jpg', 'jpeg'] }],
  });
  if (!path) return;
  await invoke('save_data_url', { dataUrl: compositedDataURL(), path });
  render();
  toast('Saved');
}

async function pinImage() {
  if (!base.width) return;
  await invoke('pin_data_url', { dataUrl: compositedDataURL() });
  render();
  toast('Pinned to screen');
}

document.getElementById('copy').onclick = copyImage;
document.getElementById('save').onclick = saveImage;
document.getElementById('pin').onclick = pinImage;

// ----------------------------------------------------------------- load ---

(async function init() {
  try {
    const url = await invoke('take_capture');
    if (!url) { emptyMsg.style.display = 'block'; return; }
    const img = new Image();
    img.onload = () => {
      base.width = img.naturalWidth; base.height = img.naturalHeight;
      bctx.drawImage(img, 0, 0);
      fitCanvas();
      render();
      updateHistoryButtons();
    };
    img.src = url;
  } catch (err) {
    emptyMsg.style.display = 'block';
    emptyMsg.textContent = 'Failed to load capture: ' + err;
  }
})();
