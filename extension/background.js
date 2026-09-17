// FullFrame Capture — companion extension.
// Full-page screenshot via Chrome DevTools Protocol:
// attach debugger -> Page.getLayoutMetrics -> Page.captureScreenshot with
// captureBeyondViewport -> detach. The browser engine renders the whole page
// in ONE shot, so there are no stitch seams and sticky headers/footers appear
// exactly once (unlike scroll-and-stitch tools).

const MAX_DIM = 16384; // Blink image-buffer guardrail

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
    let { width, height } = metrics.contentSize;
    width = Math.ceil(width);
    height = Math.ceil(height);
    let truncated = false;
    if (height > MAX_DIM) {
      height = MAX_DIM;
      truncated = true;
    }
    if (width > MAX_DIM) {
      width = MAX_DIM;
      truncated = true;
    }

    setStatus('Rendering full page…');
    const shot = await sendCommand(target, 'Page.captureScreenshot', {
      format: 'png',
      fromSurface: true,
      captureBeyondViewport: true,
      clip: { x: 0, y: 0, width, height, scale: 1 },
    });

    const stamp = new Date().toISOString().slice(0, 19).replace(/[-:T]/g, '');
    await chrome.downloads.download({
      url: 'data:image/png;base64,' + shot.data,
      filename: `fullframe-page-${stamp}.png`,
      saveAs: false,
    });
    setStatus(truncated ? 'Saved (page was taller than 16k px — truncated).' : 'Saved!');
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
