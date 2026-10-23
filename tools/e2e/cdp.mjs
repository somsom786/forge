// Minimal Chrome-DevTools-Protocol driver for the real FORGE window (WebView2 started with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222). Used for end-to-end checks of the real UI against
// the real Rust core; nothing here fakes backend behaviour.
import { writeFileSync } from 'node:fs';

export async function connect(port = 9222) {
  const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = targets.find((t) => t.type === 'page' && /localhost:1420|tauri\.localhost/.test(t.url)) ?? targets.find((t) => t.type === 'page');
  if (!page) throw new Error('no page target; is the app running with remote debugging?');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  let id = 0;
  const pending = new Map();
  const consoleLog = [];
  const dialogs = [];
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) { const { res, rej } = pending.get(msg.id); pending.delete(msg.id); msg.error ? rej(new Error(msg.error.message)) : res(msg.result); }
    else if (msg.method === 'Page.javascriptDialogOpening') { dialogs.push(`${msg.params.type}: ${msg.params.message}`); send('Page.handleJavaScriptDialog', { accept: true }).catch(() => {}); }
    else if (msg.method === 'Runtime.consoleAPICalled') consoleLog.push(`${msg.params.type}: ${msg.params.args.map((a) => a.value ?? a.description).join(' ')}`);
    else if (msg.method === 'Runtime.exceptionThrown') { const d = msg.params.exceptionDetails; const e = d.exception ?? {}; consoleLog.push(`EXCEPTION: ${e.description ?? (e.value !== undefined ? JSON.stringify(e.value) : d.text)} @ ${(d.stackTrace?.callFrames ?? []).slice(0, 3).map((f) => `${f.functionName || '?'}:${f.url.split('/').pop()}:${f.lineNumber}`).join(' < ')}`); }
  };
  const send = (method, params = {}) => new Promise((res, rej) => { const i = ++id; pending.set(i, { res, rej }); ws.send(JSON.stringify({ id: i, method, params })); });
  await send('Runtime.enable');
  await send('Page.enable');

  const api = {
    consoleLog,
    dialogs,
    send,
    /** Evaluate an expression in the page; awaits promises; returns the JSON value. */
    async eval(expression) {
      const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
      return r.result.value;
    },
    /** Call a Tauri command exactly as the UI does. */
    invoke: (cmd, args = {}) => api.eval(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`),
    async screenshot(path, { width = 1440, height = 900 } = {}) {
      await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
      const r = await send('Page.captureScreenshot', { format: 'png' });
      writeFileSync(path, Buffer.from(r.data, 'base64'));
      return path;
    },
    async waitFor(expression, { timeout = 15000, every = 150 } = {}) {
      const t0 = Date.now();
      for (;;) {
        try { const v = await api.eval(expression); if (v) return v; } catch { /* page may be navigating */ }
        if (Date.now() - t0 > timeout) throw new Error(`timed out waiting for: ${expression}`);
        await new Promise((r) => setTimeout(r, every));
      }
    },
    /** Click the first visible element whose text matches (button, a, [role=tab|button|menuitem], li). */
    async clickText(text, scope = 'button, a, [role=tab], [role=button], [role=menuitem], [role=option], li, label') {
      const ok = await api.eval(`(() => {
        const want = ${JSON.stringify(text)}.toLowerCase();
        const els = [...document.querySelectorAll(${JSON.stringify(scope)})].filter(e => e.offsetParent !== null && !e.disabled && (e.innerText || e.textContent || '').trim().toLowerCase().includes(want));
        if (!els.length) return false; els[0].scrollIntoView({block:'center'}); els[0].click(); return true; })()`);
      if (!ok) throw new Error(`no clickable element with text "${text}"`);
    },
    /** A real mouse click at the centre of the element chosen by `finder` (a function returning {x,y} or null). */
    async realClick(finder, ...args) {
      const pt = await api.call(finder, ...args);
      if (!pt) return false;
      for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) {
        await send('Input.dispatchMouseEvent', { type, x: pt.x, y: pt.y, button: type === 'mouseMoved' ? 'none' : 'left', buttons: type === 'mousePressed' ? 1 : 0, clickCount: type === 'mouseMoved' ? 0 : 1 });
      }
      return true;
    },
    /** Run a real function in the page (no string-escaping hazards). */
    call(fn, ...args) {
      return api.eval(`(${fn.toString()})(...${JSON.stringify(args)})`);
    },
    /** Click a tab by its label (tabs may carry a count badge: "Approvals 1"). */
    async clickTab(name) {
      const ok = await api.realClick((want) => {
        const t = [...document.querySelectorAll('[role=tab]')].find((e) => (e.innerText || '').trim().toLowerCase().startsWith(want.toLowerCase()));
        if (!t) return null;
        t.scrollIntoView({ block: 'center' });
        const r = t.getBoundingClientRect();
        return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
      }, name);
      if (!ok) throw new Error(`no tab "${name}"`);
      await new Promise((r) => setTimeout(r, 250));
    },
    /** Click an enabled button whose visible text equals `text` (ignoring case, a trailing shortcut hint and an ellipsis). */
    async clickButton(text, scope = 'body') {
      const ok = await api.realClick((want, scope) => {
        const clean = (t) => t.trim().replace(/…|\.\.\./g, '').trim().toLowerCase();
        const w = want.toLowerCase();
        const root = document.querySelector(scope) || document;
        const b = [...root.querySelectorAll('button')].find((e) => e.offsetParent !== null && !e.disabled && (clean(e.innerText || '') === w || clean(e.innerText || '').replace(/\s+(ctrl\+enter|[a-z])$/i, '') === w));
        if (!b) return null;
        b.scrollIntoView({ block: 'center' });
        const r = b.getBoundingClientRect();
        return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
      }, text, scope);
      if (!ok) throw new Error(`no enabled button "${text}" in ${scope}`);
      await new Promise((r) => setTimeout(r, 250));
    },
    /** Select the radio whose surrounding card text contains `text`; verifies it became checked. */
    async clickRadio(text) {
      const ok = await api.call((want) => {
        for (const r of document.querySelectorAll('input[type=radio]')) {
          const card = r.closest('label') || r.parentElement;
          if ((card.innerText || '').toLowerCase().includes(want.toLowerCase())) { r.click(); return r.checked; }
        }
        return false;
      }, text);
      if (!ok) throw new Error(`could not select radio "${text}"`);
    },
    /** Choose an <option> (by value or visible text) in the first <select> inside `scope`. */
    async selectOption(scope, valueOrText) {
      const ok = await api.call((scope, want) => {
        const sel = document.querySelector(scope + ' select') || document.querySelector(scope);
        if (!sel) return false;
        const o = [...sel.options].find((x) => x.value === want || x.text.includes(want));
        if (!o) return false;
        sel.value = o.value;
        sel.dispatchEvent(new Event('change', { bubbles: true }));
        return true;
      }, scope, valueOrText);
      if (!ok) throw new Error(`no option "${valueOrText}" in ${scope}`);
    },
    async type(selector, text) {
      await api.eval(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) throw new Error('no element ${selector}'); el.focus();
        const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
        Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(text)}); el.dispatchEvent(new Event('input', { bubbles: true })); })()`);
    },
    async text() { return api.eval('document.body.innerText'); },
    close() { ws.close(); },
  };
  return api;
}
