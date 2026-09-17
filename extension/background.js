// FullFrame Capture — companion extension.
// Full-page screenshot via Chrome DevTools Protocol:
// attach debugger -> Page.getLayoutMetrics -> Page.captureScreenshot with
// captureBeyondViewport -> detach. The browser engine renders the page itself,
// so there are no stitch seams and sticky headers/footers appear exactly once
// (unlike scroll-and-stitch tools). Pages taller than Blink's 16384px single-
// capture guardrail are captured in tiles and stitched with OffscreenCanvas —
// never silently truncated.

const SINGLE_MAX = 16384; // Blink single-capture guardrail
const TILE = 16000;       // per-tile size, safely under the guardrail
const CANVAS_MAX = 32767; // canvas dimension ceiling for the stitched result

function sendCommand(target, method, params) {
  return new Promise((resolve, reject) => {
    chrome.debugger.sendCommand(target, method, params || {}, (result) => {
      if (chrome.runtime.lastError) reject(new Error(chrome.runtime.lastError.message));
      else resolve(result);
    });
  });
}

function setStatus(msg) {
  chrome.runtime.sendMessage({ kind: 'status', msg }).catch(() => {});
}

// Capture the page in tiles (each under Blink's guardrail) and stitch them
// into one image with OffscreenCanvas. Used when a dimension exceeds what
// a single captureScreenshot can render.
async function captureTiled(target, width, height) {
  const canvas = new OffscreenCanvas(width, height);
  const ctx = canvas.getContext('2d');
  const total = Math.ceil(width / TILE) * Math.ceil(height / TILE);
  let n = 0;
  for (let y = 0; y < height; y += TILE) {
    const h = Math.min(TILE, height - y);
    for (let x = 0; x < width; x += TILE) {
      const w = Math.min(TILE, width - x);
      n++;
      setStatus(`Rendering part ${n} of ${total}…`);
      const shot = await sendCommand(target, 'Page.captureScreenshot', {
        format: 'png',
        fromSurface: true,
        captureBeyondViewport: true,
        clip: { x, y, width: w, height: h, scale: 1 },
      });
      const blob = await (await fetch('data:image/png;base64,' + shot.data)).blob();
      const bmp = await createImageBitmap(blob);
      ctx.drawImage(bmp, x, y, w, h);
      bmp.close();
    }
  }
  return canvas.convertToBlob({ type: 'image/png' });
}

function blobToDataURL(blob) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result);
    r.onerror = () => reject(new Error('Could not encode the stitched image.'));
    r.readAsDataURL(blob);
  });
}

// Scroll through the page in steps so lazy-loaded images/iframes render
// before capture. Runs in the page.
function preScroll() {
  return new Promise((resolve) => {
    const step = Math.ceil(window.innerHeight * 0.9);
    const max = document.documentElement.scrollHeight;
    let y = 0;
    const t = setInterval(() => {
      y += step;
      window.scrollTo(0, y);
      if (y >= max) {
        clearInterval(t);
        resolve(true);
      }
    }, 120);
    // Safety: never scroll longer than ~15s (infinite-scroll pages).
    setTimeout(() => { clearInterval(t); resolve(false); }, 15000);
  });
}

async function captureFullPage(tab) {
  if (!tab || tab.id === undefined) throw new Error('No active tab.');
  if (/^chrome:|^edge:|^about:|^devtools:/.test(tab.url || '')) {
    throw new Error('Browser system pages cannot be captured. Try a normal web page.');
  }
  const target = { tabId: tab.id };

  try {
    await new Promise((resolve, reject) => {
      chrome.debugger.attach(target, '1.3', () => {
        if (chrome.runtime.lastError) reject(new Error(chrome.runtime.lastError.message));
        else resolve();
      });
    });
  } catch (err) {
    // A debugger may already be attached (e.g. DevTools open). Detach and retry once.
    await new Promise((r) => chrome.debugger.detach(target, r));
    await new Promise((resolve, reject) => {
      chrome.debugger.attach(target, '1.3', () => {
        if (chrome.runtime.lastError) reject(new Error(chrome.runtime.lastError.message));
        else resolve();
      });
    });
  }

  try {
    setStatus('Waking up lazy images…');
    await chrome.scripting.executeScript({ target: { tabId: tab.id }, func: preScroll });
    await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: () => window.scrollTo(0, 0),
    });
    await new Promise((r) => setTimeout(r, 350));

    setStatus('Measuring page…');
    const metrics = await sendCommand(target, 'Page.getLayoutMetrics');
    let width = Math.ceil(metrics.contentSize.width);
    let height = Math.ceil(metrics.contentSize.height);
    let capped = false;
    if (width > CANVAS_MAX) { width = CANVAS_MAX; capped = true; }
    if (height > CANVAS_MAX) { height = CANVAS_MAX; capped = true; }

    let dataUrl;
    if (width <= SINGLE_MAX && height <= SINGLE_MAX) {
      setStatus('Rendering full page…');
      const shot = await sendCommand(target, 'Page.captureScreenshot', {
        format: 'png',
        fromSurface: true,
        captureBeyondViewport: true,
        clip: { x: 0, y: 0, width, height, scale: 1 },
      });
      dataUrl = 'data:image/png;base64,' + shot.data;
    } else {
      // Very long page: tile it and stitch. No silent truncation.
      const blob = await captureTiled(target, width, height);
      dataUrl = await blobToDataURL(blob);
    }

    const stamp = new Date().toISOString().slice(0, 19).replace(/[-:T]/g, '');
    await chrome.downloads.download({
      url: dataUrl,
      filename: `fullframe-page-${stamp}.png`,
      saveAs: false,
    });
    setStatus(capped ? 'Saved (page capped at 32767 px).' : 'Saved!');
  } finally {
    await new Promise((r) => chrome.debugger.detach(target, r));
  }
}

chrome.action.onClicked.addListener(async (tab) => {
  try {
    await captureFullPage(tab);
  } catch (err) {
    setStatus('Failed: ' + err.message);
  }
});

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg && msg.kind === 'capture') {
    chrome.tabs.query({ active: true, currentWindow: true }, async (tabs) => {
      try {
        await captureFullPage(tabs[0]);
        sendResponse({ ok: true });
      } catch (err) {
        sendResponse({ ok: false, error: err.message });
      }
    });
    return true; // async response
  }
});
