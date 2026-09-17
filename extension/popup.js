const btn = document.getElementById('go');
const status = document.getElementById('status');

chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.kind === 'status') status.textContent = msg.msg;
});

btn.onclick = async () => {
  btn.disabled = true;
  status.textContent = 'Starting…';
  try {
    const res = await chrome.runtime.sendMessage({ kind: 'capture' });
    if (!res.ok) status.textContent = 'Failed: ' + res.error;
  } catch (err) {
    status.textContent = 'Failed: ' + err.message;
  }
  btn.disabled = false;
};
