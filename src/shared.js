// Shared tiny helpers for FullFrame frontend pages.
const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args || {});
const listen = (event, handler) => window.__TAURI__.event.listen(event, handler);

function toast(msg) {
  let el = document.getElementById('toast');
  if (!el) {
    el = document.createElement('div');
    el.id = 'toast';
    document.body.appendChild(el);
  }
  el.textContent = msg;
  el.classList.add('show');
  clearTimeout(el._t);
  el._t = setTimeout(() => el.classList.remove('show'), 1800);
}
