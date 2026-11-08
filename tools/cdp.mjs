// KÃ¼Ã§Ã¼k CDP istemcisi (test aracÄ±): node tools/cdp.mjs <port> eval "<js>" | shot <dosya.png>
const [port, cmd, arg] = process.argv.slice(2);
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const t = targets.find((x) => x.type === 'page');
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0;
const pending = new Map();
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) pending.get(d.id)(d);
};
const send = (method, params = {}) => new Promise((res) => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
if (cmd === 'eval') {
  const r = await send('Runtime.evaluate', { expression: arg, awaitPromise: true, returnByValue: true });
  console.log(JSON.stringify(r.result?.result?.value ?? r.result));
} else if (cmd === 'shot') {
  const r = await send('Page.captureScreenshot', { format: 'png' });
  (await import('node:fs')).writeFileSync(arg, Buffer.from(r.result.data, 'base64'));
  console.log('ok');
} else if (cmd === 'drag') {
  const [x1, y1, x2, y2] = arg.split(',').map(Number);
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x1, y: y1 });
  await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: x1, y: y1, button: 'left', buttons: 1, clickCount: 1 });
  for (let i = 1; i <= 12; i++) await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x1 + ((x2 - x1) * i) / 12, y: y1 + ((y2 - y1) * i) / 12, button: 'left', buttons: 1 });
  await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: x2, y: y2, button: 'left', clickCount: 1 });
  console.log('dragged');
} else if (cmd === 'key') {
  for (const type of ['keyDown', 'keyUp']) await send('Input.dispatchKeyEvent', { type, key: arg, text: type === 'keyDown' ? arg : undefined });
  console.log('key');
} else if (cmd === 'click') {
  const [x, y] = arg.split(',').map(Number);
  for (const type of ['mousePressed', 'mouseReleased']) await send('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1 });
  console.log('clicked');
}
ws.close();
process.exit(0);

