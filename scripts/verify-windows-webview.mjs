import { spawn, spawnSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { once } from 'node:events';
import {
  access,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile,
} from 'node:fs/promises';
import os from 'node:os';
import { createServer } from 'node:net';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = path.join(
  root,
  'src-tauri',
  'target',
  'debug',
  'paqet-gui.exe',
);
const copiedSidecar = path.join(
  root,
  'src-tauri',
  'target',
  'debug',
  'paqet_windows_amd64.exe',
);
const expectedWidth = 440;
const expectedHeight = 760;
const commandTimeoutMs = 10_000;
const launchTimeoutMs = 30_000;
const workflowTimeoutMs = 15_000;
const pinnedSidecarSize = 9_775_616;
const pinnedSidecarSha256 =
  '49b377270473c223534ac1c2846d15c287863318e6fe6ee3c123f36ab97b441c';
const permittedSecretPaths = new Set([
  'app-data/config/profiles.json',
  'app-data/config/profiles.bak',
  'app-data/local/config.yaml',
]);
let secretSentinels = [];

async function availableLoopbackPort() {
  const server = createServer();
  server.unref();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  check(
    address && typeof address !== 'string',
    'Could not allocate a loopback port',
  );
  await new Promise((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
  return address.port;
}

function textSecretRepresentations(sentinel) {
  return [
    sentinel,
    Buffer.from(sentinel).toString('base64'),
    [...Buffer.from(sentinel)]
      .map((byte) => `%${byte.toString(16).padStart(2, '0').toUpperCase()}`)
      .join(''),
  ];
}

function percentEncodedPattern(sentinel) {
  const pattern = [...Buffer.from(sentinel)]
    .map(
      (byte) =>
        `%${byte
          .toString(16)
          .padStart(2, '0')
          .split('')
          .map((digit) =>
            /[a-f]/i.test(digit)
              ? `[${digit.toLowerCase()}${digit.toUpperCase()}]`
              : digit,
          )
          .join('')}`,
    )
    .join('');
  return new RegExp(pattern, 'g');
}

function fileSecretRepresentations(sentinel) {
  return [
    Buffer.from(sentinel),
    Buffer.from(sentinel, 'utf16le'),
    Buffer.from(Buffer.from(sentinel).toString('base64')),
    Buffer.from(textSecretRepresentations(sentinel)[2]),
  ];
}

function redact(value) {
  let redacted = String(value);
  for (const sentinel of secretSentinels) {
    for (const representation of textSecretRepresentations(sentinel)) {
      redacted = redacted.replaceAll(representation, '[REDACTED]');
    }
    redacted = redacted.replace(percentEncodedPattern(sentinel), '[REDACTED]');
  }
  return redacted;
}

function sanitizedError(error, message = undefined) {
  const detail =
    error instanceof AggregateError
      ? `${error.message}: ${error.errors.map((nested) => (nested instanceof Error ? nested.message : String(nested))).join('; ')}`
      : error instanceof Error
        ? error.message
        : String(error);
  return new Error(redact(message ? `${message}: ${detail}` : detail));
}

function check(condition, message) {
  if (!condition) throw new Error(message);
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function pathExists(filePath) {
  try {
    await access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function waitForPath(
  filePath,
  child,
  label,
  timeoutMs = workflowTimeoutMs,
) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`paqet exited while waiting for ${label}`);
    }
    if (await pathExists(filePath)) return;
    await delay(50);
  }
  throw new Error(`Timed out waiting for ${label}`);
}

async function waitForValue(
  client,
  expression,
  label,
  timeoutMs = workflowTimeoutMs,
) {
  const deadline = Date.now() + timeoutMs;
  let lastValue;
  while (Date.now() < deadline) {
    lastValue = await client.evaluate(expression, label);
    if (lastValue) return lastValue;
    await delay(50);
  }
  throw new Error(
    `Timed out waiting for ${label}; last value ${redact(JSON.stringify(lastValue))}`,
  );
}

function domAction(body) {
  return `(() => {
    const findButton = (label, root = document) => Array.from(root.querySelectorAll('button')).find((button) => button.textContent.trim() === label || button.getAttribute('aria-label') === label);
    const setInput = (selector, value) => {
      const input = document.querySelector(selector);
      if (!(input instanceof HTMLInputElement)) throw new Error('Missing input ' + selector);
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      setter.call(input, value);
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return input;
    };
    const setSelect = (selector, value) => {
      const select = document.querySelector(selector);
      if (!(select instanceof HTMLSelectElement)) throw new Error('Missing select ' + selector);
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set;
      setter.call(select, value);
      select.dispatchEvent(new Event('change', { bubbles: true }));
      return select;
    };
    const clickButton = (label, root = document) => {
      const button = findButton(label, root);
      if (!(button instanceof HTMLButtonElement)) throw new Error('Missing button ' + label);
      if (button.disabled) throw new Error('Disabled button ' + label);
      button.click();
      return button;
    };
    ${body}
  })()`;
}

async function createProfile(client, profile) {
  await client.evaluate(
    `(() => {
      if (document.querySelector('.server-management')) return true;
      const summary = document.querySelector('.server-summary');
      summary?.focus();
      summary?.click();
      return false;
    })()`,
    'open server management',
  );
  await waitForValue(
    client,
    `document.querySelector('.server-management')`,
    'server management',
  );
  await client.evaluate(
    domAction(`clickButton('Add server');`),
    'open new server editor',
  );
  await waitForValue(
    client,
    `document.querySelector('.profile-form button[type="submit"]')?.textContent.trim() === 'Save profile' && document.activeElement?.id === 'profile-name'`,
    'new profile editor',
  );
  await client.evaluate(
    domAction(`
      setInput('#profile-name', ${JSON.stringify(profile.name)});
      setInput('#server-host', ${JSON.stringify(profile.host)});
      setInput('#server-port', ${JSON.stringify(String(profile.port))});
      setInput('#encryption-key', ${JSON.stringify(profile.key)});
      document.querySelector('.profile-form').requestSubmit();
    `),
    `create ${profile.name} profile`,
  );
  await waitForValue(
    client,
    `(() => {
      return Array.from(document.querySelectorAll('.server-card-copy strong')).some((element) => element.textContent.trim() === ${JSON.stringify(profile.name)}) &&
        !document.querySelector('.profile-form button[type="submit"]');
    })()`,
    `${profile.name} canonical profile`,
  );
}

async function selectProfileByName(client, name) {
  await client.evaluate(
    `document.querySelector('.server-management') ? true : document.querySelector('.server-summary')?.click()`,
    'open server management for selection',
  );
  await waitForValue(
    client,
    `document.querySelector('.server-management')`,
    'server management selection',
  );
  await client.evaluate(
    domAction(`
      const card = Array.from(document.querySelectorAll('.server-card')).find((candidate) => candidate.querySelector('strong')?.textContent.trim() === ${JSON.stringify(name)});
      const button = card?.querySelector('.server-card-select');
      if (!(button instanceof HTMLButtonElement)) throw new Error('Missing server card');
      button.click();
    `),
    `select ${name} profile`,
  );
  await waitForValue(
    client,
    `document.querySelector('.server-summary-copy strong')?.textContent.trim() === ${JSON.stringify(name)} && !document.querySelector('.server-management')`,
    `${name} profile selection`,
  );
}

async function editSelectedProfile(client, profile) {
  await client.evaluate(
    `document.querySelector('.server-summary')?.click()`,
    'open server management for edit',
  );
  await waitForValue(
    client,
    `document.querySelector('.server-management')`,
    'server management edit',
  );
  await client.evaluate(
    domAction(`
      const selectedName = document.querySelector('.selected-server strong')?.textContent.trim();
      clickButton('Edit ' + selectedName);
    `),
    'open profile editor',
  );
  await waitForValue(
    client,
    `document.querySelector('.profile-form button[type="submit"]')?.textContent.trim() === 'Save changes'`,
    'profile edit form',
  );
  await client.evaluate(
    domAction(`
      setInput('#profile-name', ${JSON.stringify(profile.name)});
      setInput('#server-host', ${JSON.stringify(profile.host)});
      setInput('#server-port', ${JSON.stringify(String(profile.port))});
      setInput('#encryption-key', ${JSON.stringify(profile.key)});
      document.querySelector('.profile-form').requestSubmit();
    `),
    'save updated profile',
  );
  await waitForValue(
    client,
    `(() => {
      return Array.from(document.querySelectorAll('.server-card-copy strong')).some((element) => element.textContent.trim() === ${JSON.stringify(profile.name)}) &&
        !document.querySelector('.profile-form button[type="submit"]');
    })()`,
    'updated profile canonical UI',
  );
}

async function deleteSelectedProfile(client, name) {
  await client.evaluate(
    `document.querySelector('.server-management') ? true : document.querySelector('.server-summary')?.click()`,
    'open server management for deletion',
  );
  await waitForValue(
    client,
    `document.querySelector('.server-management')`,
    'server management deletion',
  );
  await client.evaluate(
    domAction(`clickButton(${JSON.stringify(`Delete ${name}`)});`),
    'request profile deletion',
  );
  await waitForValue(
    client,
    `document.querySelector('#dialog-title')?.textContent.trim() === ${JSON.stringify(`Delete “${name}”?`)}`,
    'profile deletion confirmation',
  );
  await client.evaluate(
    domAction(
      `clickButton('Delete profile', document.querySelector('.dialog'));`,
    ),
    'confirm profile deletion',
  );
  await waitForValue(
    client,
    `(() => {
      return !Array.from(document.querySelectorAll('.server-card-copy strong')).some((element) => element.textContent.trim() === ${JSON.stringify(name)}) &&
        !document.querySelector('.dialog');
    })()`,
    'deleted profile removal',
  );
}

function profileViewExpression() {
  return `(() => {
    return {
      names: Array.from(document.querySelectorAll('.server-card-copy strong')).map((element) => element.textContent.trim()),
      selectedName: document.querySelector('.selected-server strong')?.textContent.trim() ?? document.querySelector('.server-summary-copy strong')?.textContent.trim() ?? '',
      keyOnMain: Boolean(document.querySelector('#encryption-key'))
    };
  })()`;
}

async function fetchJson(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(2_000) });
  if (!response.ok) throw new Error(`${url} returned HTTP ${response.status}`);
  return response.json();
}

async function findDevToolsPort(directory, child) {
  const deadline = Date.now() + launchTimeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`paqet exited during launch with code ${child.exitCode}`);
    }
    const pending = [{ directory, depth: 0 }];
    while (pending.length > 0) {
      const current = pending.shift();
      let entries;
      try {
        entries = await readdir(current.directory, { withFileTypes: true });
      } catch {
        continue;
      }
      for (const entry of entries) {
        const entryPath = path.join(current.directory, entry.name);
        if (entry.isFile() && entry.name === 'DevToolsActivePort') {
          const port = Number.parseInt(
            (await readFile(entryPath, 'utf8')).split(/\r?\n/, 1)[0],
            10,
          );
          if (Number.isInteger(port) && port > 0 && port <= 65_535) {
            return port;
          }
        } else if (entry.isDirectory() && current.depth < 4) {
          pending.push({ directory: entryPath, depth: current.depth + 1 });
        }
      }
    }
    await delay(100);
  }
  throw new Error('Timed out waiting for the WebView2 DevTools port');
}

async function findPage(port, child) {
  const deadline = Date.now() + launchTimeoutMs;
  let lastError;
  let lastTargets = [];
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`paqet exited during launch with code ${child.exitCode}`);
    }
    try {
      const pages = await fetchJson(`http://127.0.0.1:${port}/json/list`);
      lastTargets = pages.map(({ title, type, url }) => ({ title, type, url }));
      const page = pages.find((candidate) => {
        if (
          candidate.type !== 'page' ||
          candidate.title !== 'PaqetGUI' ||
          typeof candidate.webSocketDebuggerUrl !== 'string'
        ) {
          return false;
        }
        try {
          return (
            new URL(candidate.url).hostname === 'tauri.localhost' &&
            new URL(candidate.webSocketDebuggerUrl).protocol === 'ws:' &&
            new URL(candidate.webSocketDebuggerUrl).hostname === '127.0.0.1' &&
            Number(new URL(candidate.webSocketDebuggerUrl).port) === port
          );
        } catch {
          return false;
        }
      });
      if (page) return page;
    } catch (error) {
      lastError = error;
    }
    await delay(100);
  }
  throw new Error(
    `Timed out waiting for the paqet WebView2 page; targets ${JSON.stringify(lastTargets)}`,
    { cause: lastError },
  );
}

class DevToolsClient {
  constructor(url) {
    this.socket = new WebSocket(url);
    this.closed = false;
    this.nextId = 1;
    this.pending = new Map();
    this.opened = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        cleanup();
        reject(new Error('WebView2 DevTools connection timed out'));
      }, commandTimeoutMs);
      const onOpen = () => {
        cleanup();
        resolve();
      };
      const onError = () => {
        cleanup();
        reject(new Error('WebView2 DevTools connection failed'));
      };
      const onClose = () => {
        cleanup();
        reject(new Error('WebView2 DevTools connection closed before opening'));
      };
      const cleanup = () => {
        clearTimeout(timeout);
        this.socket.removeEventListener('open', onOpen);
        this.socket.removeEventListener('error', onError);
        this.socket.removeEventListener('close', onClose);
      };
      this.socket.addEventListener('open', onOpen);
      this.socket.addEventListener('error', onError);
      this.socket.addEventListener('close', onClose);
    });
    this.socket.addEventListener('message', (event) => {
      const message = JSON.parse(event.data);
      if (!message.id) return;
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      clearTimeout(pending.timeout);
      if (message.error) {
        pending.reject(new Error(message.error.message));
      } else {
        pending.resolve(message.result);
      }
    });
    this.socket.addEventListener('close', () => {
      this.closed = true;
      for (const pending of this.pending.values()) {
        clearTimeout(pending.timeout);
        pending.reject(new Error('WebView2 DevTools connection closed'));
      }
      this.pending.clear();
    });
  }

  async send(method, params = {}) {
    if (this.closed) throw new Error('WebView2 DevTools connection is closed');
    await this.opened;
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`DevTools command ${method} timed out`));
      }, commandTimeoutMs);
      this.pending.set(id, { resolve, reject, timeout });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  async evaluate(expression, label = 'evaluation', options = {}) {
    let result;
    try {
      result = await this.send('Runtime.evaluate', {
        expression,
        awaitPromise: true,
        returnByValue: true,
        ...options,
      });
    } catch (error) {
      throw sanitizedError(error, `WebView ${label} failed`);
    }
    if (result.exceptionDetails) {
      throw new Error(
        redact(
          result.exceptionDetails.exception?.description ??
            result.exceptionDetails.text ??
            'WebView evaluation failed',
        ),
      );
    }
    return result.result.value;
  }

  close() {
    if (!this.closed) this.socket.close();
  }
}

async function waitForApplication(client) {
  const deadline = Date.now() + launchTimeoutMs;
  while (Date.now() < deadline) {
    const ready = await client.evaluate(
      `(async () => {
      await document.fonts.ready;
      return {
      documentReady: document.readyState === 'complete',
      fontsReady: document.fonts.status === 'loaded',
      hasShell: Boolean(document.querySelector('.app-shell')),
      hasConnection: Boolean(document.querySelector('.connect-button')),
      hasLogs: Boolean(document.querySelector('[aria-label="Log actions"]')),
      loading: document.querySelector('.configuration')?.getAttribute('aria-busy') === 'true'
      };
    })()`,
      'startup readiness',
    );
    if (
      ready.documentReady &&
      ready.fontsReady &&
      ready.hasShell &&
      ready.hasConnection &&
      ready.hasLogs &&
      !ready.loading
    ) {
      return;
    }
    await delay(100);
  }
  throw new Error('Timed out waiting for the application shell to initialize');
}

async function readMetrics(client) {
  return client.evaluate(
    `(() => {
    const root = document.scrollingElement;
    const rect = (selector) => {
      const element = document.querySelector(selector);
      if (!element) return null;
      const bounds = element.getBoundingClientRect();
      return {
        left: bounds.left,
        top: bounds.top,
        right: bounds.right,
        bottom: bounds.bottom,
        width: bounds.width,
        height: bounds.height
      };
    };
    return {
      innerWidth,
      innerHeight,
      outerWidth,
      outerHeight,
      devicePixelRatio,
      viewportScale: window.visualViewport?.scale ?? 1,
      clientWidth: root.clientWidth,
      clientHeight: root.clientHeight,
      scrollWidth: root.scrollWidth,
      scrollHeight: root.scrollHeight,
      shell: rect('.app-shell'),
      connect: rect('.connect-button'),
      logActions: rect('[aria-label="Log actions"]'),
      log: rect('[aria-label="Connection logs"]')
    };
  })()`,
    'geometry measurement',
  );
}

function assertNoHorizontalOverflow(metrics, context) {
  check(
    metrics.scrollWidth <= metrics.clientWidth + 1,
    `${context} has horizontal document overflow (${metrics.scrollWidth}px > ${metrics.clientWidth}px)`,
  );
}

async function assertReachable(client, selector, label) {
  const result = await client.evaluate(
    `(async () => {
    const element = document.querySelector(${JSON.stringify(selector)});
    if (!element) return null;
    element.scrollIntoView({ block: 'center', inline: 'nearest' });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const bounds = element.getBoundingClientRect();
    return {
      left: bounds.left,
      top: bounds.top,
      right: bounds.right,
      bottom: bounds.bottom,
      width: bounds.width,
      height: bounds.height,
      viewportWidth: innerWidth,
      viewportHeight: innerHeight
    };
  })()`,
    `${label} reachability`,
  );
  check(result, `${label} is missing`);
  check(result.width > 0 && result.height > 0, `${label} has no rendered size`);
  check(
    result.left >= -1 && result.right <= result.viewportWidth + 1,
    `${label} is not horizontally reachable`,
  );
  check(
    result.top >= -1 && result.bottom <= result.viewportHeight + 1,
    `${label} is not vertically reachable after scrolling`,
  );
}

const inputScript = String.raw`
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PaqetInput {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr hWnd, int command);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out Rect rect);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out Rect rect);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hWnd);
}
'@
$process = Get-Process -Id ([int]$env:PAQET_TEST_PID) -ErrorAction Stop
$handle = $process.MainWindowHandle
if ($handle -eq [IntPtr]::Zero) { throw 'paqet main window is unavailable' }
[PaqetInput+Rect]$client = New-Object PaqetInput+Rect
[PaqetInput+Rect]$window = New-Object PaqetInput+Rect
$mode = $env:PAQET_TEST_INPUT
if ($mode -eq 'metrics') {
  if (-not [PaqetInput]::GetClientRect($handle, [ref]$client)) { throw 'cannot read paqet client bounds' }
  if (-not [PaqetInput]::GetWindowRect($handle, [ref]$window)) { throw 'cannot read paqet window bounds' }
  @{
    clientWidth = $client.Right - $client.Left
    clientHeight = $client.Bottom - $client.Top
    windowWidth = $window.Right - $window.Left
    windowHeight = $window.Bottom - $window.Top
    dpi = [PaqetInput]::GetDpiForWindow($handle)
  } | ConvertTo-Json -Compress
  exit 0
}
if ($mode -eq 'close') {
  if (-not $process.CloseMainWindow()) { throw 'cannot close paqet main window' }
  exit 0
}
[PaqetInput]::ShowWindowAsync($handle, 9) | Out-Null
$focused = $false
for ($attempt = 0; $attempt -lt 10; $attempt++) {
  [PaqetInput]::ShowWindowAsync($handle, 9) | Out-Null
  [PaqetInput]::SetForegroundWindow($handle) | Out-Null
  Start-Sleep -Milliseconds 40
  if ([PaqetInput]::GetForegroundWindow() -eq $handle) { $focused = $true; break }
}
if (-not $focused) { throw 'cannot focus paqet main window' }
$count = [int]$env:PAQET_TEST_COUNT
$KEYUP = 2
try {
  if ($mode -eq 'zoom-in') {
    for ($index = 0; $index -lt $count; $index++) {
      if ([PaqetInput]::GetForegroundWindow() -ne $handle) { throw 'paqet lost foreground ownership' }
      [PaqetInput]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
      [PaqetInput]::keybd_event(0xBB, 0, 0, [UIntPtr]::Zero)
      [PaqetInput]::keybd_event(0xBB, 0, $KEYUP, [UIntPtr]::Zero)
      [PaqetInput]::keybd_event(0x11, 0, $KEYUP, [UIntPtr]::Zero)
      Start-Sleep -Milliseconds 80
    }
  } elseif ($mode -eq 'zoom-reset') {
    if ([PaqetInput]::GetForegroundWindow() -ne $handle) { throw 'paqet lost foreground ownership' }
    [PaqetInput]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [PaqetInput]::keybd_event(0x30, 0, 0, [UIntPtr]::Zero)
    [PaqetInput]::keybd_event(0x30, 0, $KEYUP, [UIntPtr]::Zero)
    [PaqetInput]::keybd_event(0x11, 0, $KEYUP, [UIntPtr]::Zero)
  } elseif ($mode -eq 'tab' -or $mode -eq 'shift-tab') {
    for ($index = 0; $index -lt $count; $index++) {
      if ([PaqetInput]::GetForegroundWindow() -ne $handle) { throw 'paqet lost foreground ownership' }
      if ($mode -eq 'shift-tab') { [PaqetInput]::keybd_event(0x10, 0, 0, [UIntPtr]::Zero) }
      [PaqetInput]::keybd_event(0x09, 0, 0, [UIntPtr]::Zero)
      [PaqetInput]::keybd_event(0x09, 0, $KEYUP, [UIntPtr]::Zero)
      if ($mode -eq 'shift-tab') { [PaqetInput]::keybd_event(0x10, 0, $KEYUP, [UIntPtr]::Zero) }
      Start-Sleep -Milliseconds 80
    }
  } elseif ($mode -eq 'escape') {
    if ([PaqetInput]::GetForegroundWindow() -ne $handle) { throw 'paqet lost foreground ownership' }
    [PaqetInput]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero)
    [PaqetInput]::keybd_event(0x1B, 0, $KEYUP, [UIntPtr]::Zero)
  } else {
    throw "unknown input mode $mode"
  }
} finally {
  [PaqetInput]::keybd_event(0x10, 0, $KEYUP, [UIntPtr]::Zero)
  [PaqetInput]::keybd_event(0x11, 0, $KEYUP, [UIntPtr]::Zero)
  [PaqetInput]::keybd_event(0x12, 0, $KEYUP, [UIntPtr]::Zero)
  [PaqetInput]::keybd_event(0x1B, 0, $KEYUP, [UIntPtr]::Zero)
}
`;

function sendNativeInput(child, mode, count = 1) {
  let result;
  try {
    result = spawnSync(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-Command', inputScript],
      {
        encoding: 'utf8',
        env: {
          ...process.env,
          PAQET_TEST_PID: String(child.pid),
          PAQET_TEST_INPUT: mode,
          PAQET_TEST_COUNT: String(count),
        },
        timeout: commandTimeoutMs,
      },
    );
  } finally {
    const releaseResult = spawnSync(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class KeyRelease { [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra); }'; 0x09,0x10,0x11,0x12,0x1B,0x30,0xBB | ForEach-Object { [KeyRelease]::keybd_event($_, 0, 2, [UIntPtr]::Zero) }`,
      ],
      { stdio: 'ignore', timeout: commandTimeoutMs },
    );
    check(
      releaseResult.status === 0,
      'Cannot confirm release of injected keyboard input',
    );
  }
  if (result.status !== 0) {
    throw new Error(
      redact(
        `Windows input ${mode} failed: ${(result.stderr || result.stdout).trim()}`,
      ),
    );
  }
  return result.stdout.trim();
}

function retryNativeInput(child, mode, count = 1) {
  let lastError;
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      return sendNativeInput(child, mode, count);
    } catch (error) {
      lastError = error;
      spawnSync('powershell.exe', [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        'Start-Sleep -Milliseconds 200',
      ]);
    }
  }
  throw lastError;
}

async function verifyKeyboard(client, child) {
  await client.evaluate(
    `(() => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    document.body.tabIndex = -1;
    document.body.focus();
  })()`,
    'keyboard focus reset',
  );
  const focused = [];
  const indexes = [];
  for (let index = 0; index < 20; index += 1) {
    sendNativeInput(child, 'tab');
    await delay(120);
    const descriptor = await client.evaluate(
      `(() => {
      const element = document.activeElement;
      if (!(element instanceof HTMLElement) || element === document.body) return null;
      const bounds = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      const focusable = Array.from(document.querySelectorAll('button, input, select, textarea, [tabindex]'));
      return {
        tag: element.tagName.toLowerCase(),
        id: element.id || null,
        classes: Array.from(element.classList).sort(),
        index: focusable.indexOf(element),
        width: bounds.width,
        height: bounds.height,
        visible: bounds.bottom > 0 && bounds.top < innerHeight && bounds.right > 0 && bounds.left < innerWidth,
        outlineWidth: Number.parseFloat(style.outlineWidth) || 0,
        outlineStyle: style.outlineStyle
      };
    })()`,
      `keyboard step ${index + 1}`,
    );
    check(descriptor, `Tab ${index + 1} did not focus an interactive control`);
    check(
      descriptor.visible,
      `Tab ${index + 1} focused an unreachable control`,
    );
    check(
      descriptor.outlineWidth >= 2 && descriptor.outlineStyle !== 'none',
      `Tab ${index + 1} did not expose the required focus ring`,
    );
    const label = `${descriptor.tag}[${descriptor.index}]${descriptor.id ? `#${descriptor.id}` : ''}${descriptor.classes.length ? `.${descriptor.classes.join('.')}` : ''}`;
    if (focused.includes(label)) {
      check(
        label === focused[0],
        `Tab traversal repeated out of sequence: ${[...focused, label].join(' -> ')}`,
      );
      break;
    }
    focused.push(label);
    indexes.push(descriptor.index);
  }
  check(
    focused.length === 4 &&
      focused[0].includes('.server-summary') &&
      focused[1].includes('.disclosure-button') &&
      focused[2].includes('#socks-port') &&
      focused[3].includes('.log'),
    `Unexpected disconnected focus cycle: ${focused.join(' -> ')}`,
  );
  check(
    indexes.every(
      (index, position) => position === 0 || index > indexes[position - 1],
    ),
    `Tab traversal is not in DOM/visual order: ${focused.join(' -> ')}`,
  );
  return focused;
}

async function verifyClipboardApi(client) {
  const available = await client.evaluate(
    `(() => ({
      secureContext: window.isSecureContext,
      clipboard: typeof navigator.clipboard === 'object',
      writeText: typeof navigator.clipboard?.writeText === 'function'
    }))()`,
    'clipboard API inspection',
  );
  check(available.secureContext, 'WebView2 is not a secure clipboard context');
  check(
    available.clipboard && available.writeText,
    'WebView2 clipboard.writeText is unavailable',
  );
}

async function verifyFonts(client) {
  const fonts = await client.evaluate(
    `(async () => {
      const [interfaceRegular, interfaceBold, monoRegular, monoBold] = await Promise.all([
        document.fonts.load('400 16px "Space Grotesk"', 'paqet'),
        document.fonts.load('680 25px "Space Grotesk"', 'paqet'),
        document.fonts.load('400 11px "JetBrains Mono"', 'connection output'),
        document.fonts.load('700 9px "JetBrains Mono"', 'stderr')
      ]);
      const loaded = (faces) => faces.length > 0 && faces.every((face) => face.status === 'loaded');
      const firstFamily = (value) => value.split(',')[0].trim().replace(/^['"]|['"]$/g, '');
      return {
      interfaceFont: firstFamily(getComputedStyle(document.documentElement).fontFamily),
      logFont: firstFamily(getComputedStyle(document.querySelector('.log')).fontFamily),
      interfaceLoaded: loaded(interfaceRegular) && loaded(interfaceBold),
      monoLoaded: loaded(monoRegular) && loaded(monoBold)
      };
    })()`,
    'font inspection',
  );
  check(
    fonts.interfaceLoaded && fonts.interfaceFont === 'Space Grotesk',
    'Space Grotesk is not active in WebView2',
  );
  check(
    fonts.monoLoaded && fonts.logFont === 'JetBrains Mono',
    'JetBrains Mono is not active in WebView2 logs',
  );
}

async function waitForExit(child, timeoutMs) {
  if (child.exitCode !== null) return child.exitCode;
  return Promise.race([
    once(child, 'exit').then(([code]) => code),
    delay(timeoutMs).then(() => undefined),
  ]);
}

function processIdentity(pid) {
  const result = spawnSync(
    'powershell.exe',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `$process = Get-CimInstance Win32_Process -Filter 'ProcessId = ${pid}'; if ($null -eq $process) { exit 1 }; @{ pid = [uint32]$process.ProcessId; name = $process.Name; created = [string]$process.CreationDate } | ConvertTo-Json -Compress`,
    ],
    { encoding: 'utf8', timeout: commandTimeoutMs },
  );
  if (result.error || result.signal || result.status === null) {
    throw new Error(`Cannot inspect process ${pid}`);
  }
  if (result.status === 0) return JSON.parse(result.stdout);
  if (result.status === 1) return undefined;
  throw new Error(`Process inspection failed for ${pid}`);
}

function processIdentityExists(identity) {
  const current = processIdentity(identity.pid);
  return (
    current !== undefined &&
    current.name === identity.name &&
    current.created === identity.created
  );
}

function processTreeIdentities(rootPid) {
  const result = spawnSync(
    'powershell.exe',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `$all = Get-CimInstance Win32_Process; $ids = [Collections.Generic.HashSet[uint32]]::new(); $ids.Add([uint32]${rootPid}) | Out-Null; do { $added = $false; foreach ($process in $all) { if ($ids.Contains([uint32]$process.ParentProcessId) -and $ids.Add([uint32]$process.ProcessId)) { $added = $true } } } while ($added); @($all | Where-Object { $ids.Contains([uint32]$_.ProcessId) } | ForEach-Object { @{ pid = [uint32]$_.ProcessId; name = $_.Name; created = [string]$_.CreationDate } }) | ConvertTo-Json -Compress`,
    ],
    { encoding: 'utf8', timeout: commandTimeoutMs },
  );
  check(result.status === 0, 'Cannot inspect the application process tree');
  const identities = JSON.parse(result.stdout || '[]');
  return Array.isArray(identities) ? identities : [identities];
}

function terminateProcessTree(identity) {
  if (!identity || !processIdentityExists(identity)) return;
  const result = spawnSync(
    'taskkill.exe',
    ['/PID', String(identity.pid), '/T', '/F'],
    {
      stdio: 'ignore',
      timeout: commandTimeoutMs,
    },
  );
  check(
    result.status === 0 || !processIdentityExists(identity),
    'Cannot terminate paqet tree',
  );
}

function terminateOwnedChild(session) {
  if (session.hostIdentity) {
    terminateProcessTree(session.hostIdentity);
    return;
  }
  if (session.child.exitCode !== null) return;
  const result = spawnSync(
    'taskkill.exe',
    ['/PID', String(session.child.pid), '/T', '/F'],
    { stdio: 'ignore', timeout: commandTimeoutMs },
  );
  check(
    result.status === 0 || session.child.exitCode !== null,
    'Cannot terminate the unidentified application process tree',
  );
}

async function waitForTestProcesses(identities, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (identities.every((identity) => !processIdentityExists(identity)))
      return;
    await delay(100);
  }
  const remaining = identities.filter(processIdentityExists);
  throw new Error(
    `Application processes remained: ${remaining.map(({ pid, name }) => `${name}:${pid}`).join(', ')}`,
  );
}

async function waitForPaqetProcess(session, label) {
  const deadline = Date.now() + workflowTimeoutMs;
  while (Date.now() < deadline) {
    if (session.child.exitCode !== null) {
      throw new Error(`paqet exited while waiting for ${label}`);
    }
    const identity = processTreeIdentities(session.child.pid).find(
      ({ name, pid }) =>
        pid !== session.child.pid &&
        name.toLowerCase() === 'paqet_windows_amd64.exe',
    );
    if (identity) {
      session.identities.push(identity);
      return identity;
    }
    await delay(50);
  }
  throw new Error(`Timed out waiting for ${label}`);
}

function terminateExactProcess(identity) {
  check(
    processIdentityExists(identity),
    'The expected paqet process is absent',
  );
  const result = spawnSync(
    'taskkill.exe',
    ['/PID', String(identity.pid), '/F'],
    { stdio: 'ignore', timeout: commandTimeoutMs },
  );
  check(
    result.status === 0 || !processIdentityExists(identity),
    'Cannot terminate the exact paqet process',
  );
}

function wrongIdentitySidecar(exactSidecar) {
  const mutated = Buffer.from(exactSidecar);
  const peOffset = mutated.readUInt32LE(0x3c);
  check(
    mutated.subarray(peOffset, peOffset + 4).equals(Buffer.from('PE\0\0')),
    'The pinned sidecar does not have a valid PE signature',
  );
  const optionalHeader = peOffset + 24;
  check(
    [0x10b, 0x20b].includes(mutated.readUInt16LE(optionalHeader)),
    'The pinned sidecar has an unsupported PE optional header',
  );
  const checksumOffset = optionalHeader + 64;
  mutated.writeUInt32LE(
    (mutated.readUInt32LE(checksumOffset) ^ 0xffffffff) >>> 0,
    checksumOffset,
  );
  return mutated;
}

function verifyWrongIdentityRemainsLaunchable() {
  const result = spawnSync(copiedSidecar, ['version'], {
    encoding: 'utf8',
    timeout: commandTimeoutMs,
    windowsHide: true,
  });
  check(
    result.status === 0 && result.signal === null && !result.error,
    'The wrong-identity sidecar is not independently launchable',
  );
}

function uniqueIdentities(identities) {
  return [
    ...new Map(
      identities
        .filter(Boolean)
        .map((identity) => [
          `${identity.pid}:${identity.name}:${identity.created}`,
          identity,
        ]),
    ).values(),
  ];
}

async function launchApplication(
  testRoot,
  appData,
  launchNumber,
  extraEnv,
  sessions,
) {
  const userData = path.join(testRoot, `webview-launch-${launchNumber}`);
  const child = spawn(executable, [], {
    cwd: root,
    env: {
      ...process.env,
      PAQET_GUI_TEST_DATA_DIR: appData,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:
        '--remote-debugging-address=127.0.0.1 --remote-debugging-port=0',
      WEBVIEW2_USER_DATA_FOLDER: userData,
      ...extraEnv,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const session = {
    appOutput: '',
    child,
    client: undefined,
    hostIdentity: undefined,
    identities: [],
    outputTails: { stderr: '', stdout: '' },
    pageUrl: undefined,
    streamsClosed: Promise.all([
      once(child.stdout, 'close'),
      once(child.stderr, 'close'),
    ]),
    userData,
    version: undefined,
  };
  sessions.add(session);
  const captureOutput = (stream) => (chunk) => {
    const text = chunk.toString();
    const searchable = `${session.outputTails[stream]}${text}`;
    if (
      secretSentinels.some((sentinel) => {
        const [exact, base64, percent] = textSecretRepresentations(sentinel);
        return (
          searchable.includes(exact) ||
          searchable.includes(base64) ||
          searchable.toLowerCase().includes(percent.toLowerCase())
        );
      })
    ) {
      session.outputSecretLeak = true;
    }
    session.outputTails[stream] = searchable.slice(-512);
    session.appOutput = `${session.appOutput}${redact(text)}`.slice(
      -128 * 1024,
    );
  };
  child.stdout.on('data', captureOutput('stdout'));
  child.stderr.on('data', captureOutput('stderr'));

  try {
    session.hostIdentity = processIdentity(child.pid);
    check(session.hostIdentity, 'Cannot identify the launched paqet process');
    session.identities.push(session.hostIdentity);
    const port = await findDevToolsPort(userData, child);
    const page = await findPage(port, child);
    session.client = new DevToolsClient(page.webSocketDebuggerUrl);
    session.pageUrl = page.url;
    await waitForApplication(session.client);
    session.version = await session.client.send('Browser.getVersion');
    session.identities.push(...processTreeIdentities(child.pid));
    return session;
  } catch (error) {
    const teardownErrors = [];
    try {
      terminateOwnedChild(session);
    } catch (teardownError) {
      teardownErrors.push(sanitizedError(teardownError));
    }
    try {
      await waitForTestProcesses(uniqueIdentities(session.identities), 5_000);
    } catch (teardownError) {
      teardownErrors.push(sanitizedError(teardownError));
    }
    try {
      await Promise.race([
        session.streamsClosed,
        delay(2_000).then(() => {
          throw new Error(
            'Application output pipes did not close after launch failure',
          );
        }),
      ]);
    } catch (teardownError) {
      teardownErrors.push(sanitizedError(teardownError));
    }
    if (teardownErrors.length > 0) {
      throw new AggregateError(
        [
          sanitizedError(error, `Launch ${launchNumber} failed`),
          ...teardownErrors,
        ],
        `Launch ${launchNumber} and teardown failed`,
        { cause: error },
      );
    }
    throw sanitizedError(error, `Launch ${launchNumber} failed`);
  }
}

async function closeApplication(session) {
  const errors = [];
  session.client?.close();
  if (session.child.exitCode === null) {
    try {
      session.identities.push(...processTreeIdentities(session.child.pid));
    } catch (error) {
      errors.push(sanitizedError(error));
    }
  }
  if (session.child.exitCode === null) {
    try {
      sendNativeInput(session.child, 'close');
    } catch (error) {
      errors.push(sanitizedError(error));
      try {
        terminateOwnedChild(session);
      } catch (killError) {
        errors.push(sanitizedError(killError));
      }
    }
  }
  const exitCode = await waitForExit(session.child, 5_000);
  if (exitCode === undefined && session.child.exitCode === null) {
    errors.push(new Error('paqet did not exit after graceful window close'));
    try {
      terminateOwnedChild(session);
    } catch (error) {
      errors.push(sanitizedError(error));
    }
    if ((await waitForExit(session.child, 2_000)) === undefined) {
      errors.push(new Error('paqet did not exit after forced teardown'));
    }
  }
  try {
    await waitForTestProcesses(uniqueIdentities(session.identities), 5_000);
  } catch (error) {
    errors.push(sanitizedError(error));
  }
  try {
    await Promise.race([
      session.streamsClosed,
      delay(2_000).then(() => {
        throw new Error(
          'Application output pipes did not close during teardown',
        );
      }),
    ]);
  } catch (error) {
    errors.push(sanitizedError(error));
  }
  if (exitCode !== undefined && exitCode !== 0) {
    errors.push(
      new Error(`paqet exited during teardown with code ${exitCode}`),
    );
  }
  if (session.outputSecretLeak) {
    errors.push(new Error('Application output contained a secret sentinel'));
  }
  if (errors.length > 0) {
    throw new AggregateError(errors, 'Application launch teardown failed');
  }
}

async function closeActiveApplication(session, paqetIdentity) {
  session.identities.push(...processTreeIdentities(session.child.pid));
  sendNativeInput(session.child, 'close');
  await waitForValue(
    session.client,
    `document.querySelector('#dialog-title')?.textContent.trim() === 'Disconnect and close?'`,
    'supervised close confirmation',
  );
  check(
    processIdentityExists(paqetIdentity),
    'paqet exited before supervised close confirmation',
  );
  await session.client.evaluate(
    domAction(
      `clickButton('Disconnect and close', document.querySelector('.dialog'));`,
    ),
    'confirm supervised close',
  );
  const exitCode = await waitForExit(session.child, workflowTimeoutMs);
  check(
    exitCode === 0,
    `paqet exited during supervised close with code ${exitCode}`,
  );
  session.client.close();
  await waitForTestProcesses(uniqueIdentities(session.identities), 5_000);
  await Promise.race([
    session.streamsClosed,
    delay(2_000).then(() => {
      throw new Error(
        'Application output pipes did not close after supervised close',
      );
    }),
  ]);
  check(
    !session.outputSecretLeak,
    'Application output contained a secret sentinel',
  );
}

async function forceCloseApplication(session) {
  const errors = [];
  session.client?.close();
  if (session.child.exitCode === null) {
    try {
      session.identities.push(...processTreeIdentities(session.child.pid));
    } catch (error) {
      errors.push(sanitizedError(error));
    }
    try {
      terminateOwnedChild(session);
    } catch (error) {
      errors.push(sanitizedError(error));
    }
  }
  try {
    await waitForExit(session.child, 5_000);
    await waitForTestProcesses(uniqueIdentities(session.identities), 5_000);
  } catch (error) {
    errors.push(sanitizedError(error));
  }
  try {
    await Promise.race([
      session.streamsClosed,
      delay(2_000).then(() => {
        throw new Error(
          'Application output pipes did not close after forced cleanup',
        );
      }),
    ]);
  } catch (error) {
    errors.push(sanitizedError(error));
  }
  if (errors.length > 0) {
    throw new AggregateError(errors, 'Application forced cleanup failed');
  }
}

async function verifyApp001A(session) {
  const { child, client } = session;
  const baseline = await readMetrics(client);
  const nativeMetrics = JSON.parse(sendNativeInput(child, 'metrics'));
  check(
    baseline.innerWidth === expectedWidth &&
      baseline.innerHeight === expectedHeight,
    `Expected ${expectedWidth}x${expectedHeight} logical client area, received ${baseline.innerWidth}x${baseline.innerHeight}`,
  );
  assertNoHorizontalOverflow(baseline, 'Default zoom');
  check(
    nativeMetrics.clientWidth ===
      Math.round(expectedWidth * (nativeMetrics.dpi / 96)) &&
      nativeMetrics.clientHeight ===
        Math.round(expectedHeight * (nativeMetrics.dpi / 96)),
    `Native client geometry does not match ${expectedWidth}x${expectedHeight} logical pixels at ${nativeMetrics.dpi} DPI`,
  );
  check(
    Math.abs(baseline.devicePixelRatio - nativeMetrics.dpi / 96) <= 0.01 &&
      Math.abs(baseline.viewportScale - 1) <= 0.01,
    `WebView scaling does not match ${nativeMetrics.dpi} DPI`,
  );
  check(
    baseline.connect?.height >= 48,
    'Primary connection action is smaller than 48 CSS pixels',
  );
  const focused = await verifyKeyboard(client, child);

  await client.evaluate(
    `(() => {
      window.__paqetZoomKeys = [];
      addEventListener('keydown', (event) => {
        window.__paqetZoomKeys.push({ key: event.key, code: event.code, ctrlKey: event.ctrlKey });
      });
    })()`,
    'zoom key observer setup',
  );
  sendNativeInput(child, 'zoom-in', 5);
  await delay(500);
  const zoomed = await readMetrics(client);
  const zoomRatio = zoomed.devicePixelRatio / baseline.devicePixelRatio;
  const zoomKeys = await client.evaluate(
    'window.__paqetZoomKeys',
    'zoom key evidence',
  );
  check(
    Math.abs(zoomRatio - 2) <= 0.05,
    `Expected 200% browser zoom, received ${Math.round(zoomRatio * 100)}%; keys ${JSON.stringify(zoomKeys)}; viewport ${baseline.innerWidth}x${baseline.innerHeight} -> ${zoomed.innerWidth}x${zoomed.innerHeight}`,
  );
  assertNoHorizontalOverflow(zoomed, '200% zoom');
  await assertReachable(client, '.connect-button', 'Primary connection action');
  await assertReachable(client, '[aria-label="Log actions"]', 'Log actions');
  await assertReachable(
    client,
    '[aria-label="Connection logs"]',
    'Log surface',
  );
  await verifyClipboardApi(client);
  await verifyFonts(client);
  session.identities.push(...processTreeIdentities(child.pid));
  sendNativeInput(child, 'zoom-reset');
  await delay(300);
  const restored = await readMetrics(client);
  check(
    Math.abs(restored.devicePixelRatio - baseline.devicePixelRatio) <= 0.01,
    'Browser zoom did not return to 100%',
  );
  return { baseline, focused, nativeMetrics, zoomed, zoomRatio };
}

function axValue(node, property) {
  return node[property]?.value;
}

function axProperty(node, property) {
  return node.properties?.find((candidate) => candidate.name === property)
    ?.value?.value;
}

async function readAccessibilityTree(client) {
  const { nodes } = await client.send('Accessibility.getFullAXTree');
  return nodes;
}

function findAxNode(nodes, role, name, exact = true) {
  return nodes.find(
    (node) =>
      !node.ignored &&
      axValue(node, 'role') === role &&
      (exact
        ? axValue(node, 'name') === name
        : axValue(node, 'name')?.startsWith(name)),
  );
}

async function verifyAccessibilityShell(client) {
  await client.send('Accessibility.enable');
  const nodes = await readAccessibilityTree(client);
  const requiredNodes = [
    ['heading', 'PaqetGUI', true],
    ['status', 'Connection status: Disconnected', true],
    ['button', 'Manage servers.', false],
    ['button', 'Advanced', false],
    ['textbox', 'SOCKS port', true],
    ['button', 'Connect', true],
    ['log', 'Connection logs', true],
  ];
  for (const [role, name, exact] of requiredNodes) {
    check(
      findAxNode(nodes, role, name, exact),
      `Accessibility tree is missing ${role} “${name}”`,
    );
  }
  const disclosure = findAxNode(nodes, 'button', 'Advanced', false);
  check(
    axProperty(disclosure, 'expanded') === false,
    'Advanced accessibility state is not collapsed',
  );
}

async function verifyEmulatedPreferences(client, child) {
  await client.send('Emulation.setEmulatedMedia', {
    media: '',
    features: [
      { name: 'forced-colors', value: 'active' },
      { name: 'prefers-reduced-motion', value: 'reduce' },
    ],
  });
  const preferences = await client.evaluate(
    `(() => {
      const chevron = document.querySelector('.chevron');
      const surfaces = ['.configuration', '.connection', '.log', 'button'];
      return {
        forcedColors: matchMedia('(forced-colors: active)').matches,
        reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches,
        transitionDurations: chevron ? getComputedStyle(chevron).transitionDuration.split(',').map((value) => Number.parseFloat(value) || 0) : [],
        boundaries: surfaces.map((selector) => {
          const element = document.querySelector(selector);
          const style = element ? getComputedStyle(element) : null;
          return {
            selector,
            width: style ? Number.parseFloat(style.borderTopWidth) || 0 : 0,
            style: style?.borderTopStyle ?? 'none'
          };
        }),
        systemIndicators: ['.wordmark-mark', '.status-indicator', '.connect-button span'].map((selector) => getComputedStyle(document.querySelector(selector)).forcedColorAdjust)
      };
    })()`,
    'emulated accessibility preferences',
  );
  check(preferences.forcedColors, 'Forced-colors emulation did not activate');
  check(preferences.reducedMotion, 'Reduced-motion emulation did not activate');
  check(
    preferences.transitionDurations.length > 0 &&
      preferences.transitionDurations.every((duration) => duration <= 0.001),
    'Reduced-motion transition override is not effective',
  );
  check(
    preferences.boundaries.every(
      (boundary) => boundary.width >= 1 && boundary.style !== 'none',
    ),
    'Forced-colors mode removed a required control or surface boundary',
  );
  check(
    preferences.systemIndicators.every((value) => value !== 'none'),
    'Forced-color indicators are not adapting to system colors',
  );

  await client.evaluate(
    `(() => {
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      document.body.tabIndex = -1;
      document.body.focus();
    })()`,
    'forced-colors focus reset',
  );
  sendNativeInput(child, 'tab');
  await delay(120);
  const focus = await client.evaluate(
    `(() => {
      const element = document.activeElement;
      const style = element instanceof HTMLElement ? getComputedStyle(element) : null;
      return {
        text: element?.textContent?.trim() ?? '',
        classes: Array.from(element?.classList ?? []),
        outlineWidth: style ? Number.parseFloat(style.outlineWidth) || 0 : 0,
        outlineStyle: style?.outlineStyle ?? 'none'
      };
    })()`,
    'forced-colors focus ring',
  );
  check(
    focus.classes.includes('server-summary') &&
      focus.outlineWidth >= 2 &&
      focus.outlineStyle !== 'none',
    'Forced-colors keyboard focus ring is missing or out of order',
  );
  assertNoHorizontalOverflow(await readMetrics(client), 'Forced-colors mode');
}

async function focusedButtonText(client) {
  return client.evaluate(
    `document.activeElement instanceof HTMLButtonElement ? document.activeElement.textContent.trim() : ''`,
    'focused dialog button',
  );
}

async function verifyDialogAccessibility(client, child, expected) {
  await waitForValue(
    client,
    `document.querySelector('#dialog-title')?.textContent.trim() === ${JSON.stringify(expected.title)}`,
    `${expected.label} dialog`,
  );
  const nodes = await readAccessibilityTree(client);
  const dialog = findAxNode(nodes, 'alertdialog', expected.title);
  check(
    dialog,
    `${expected.label} dialog is missing from the accessibility tree`,
  );
  check(
    axValue(dialog, 'description') === expected.description,
    `${expected.label} dialog description is not accessible`,
  );
  check(
    axProperty(dialog, 'modal') === true,
    `${expected.label} dialog is not exposed as modal`,
  );
  check(
    [
      ['heading', 'PaqetGUI'],
      ['region', 'Servers'],
      ['button', 'Connect'],
      ['log', 'Connection logs'],
    ].every(([role, name]) => !findAxNode(nodes, role, name)),
    `${expected.label} dialog did not hide the application shell from accessibility`,
  );
  check(
    (await focusedButtonText(client)) === expected.initialFocus,
    `${expected.label} dialog initial focus is incorrect`,
  );
  const focusedNode = findAxNode(nodes, 'button', expected.initialFocus);
  check(
    focusedNode && axProperty(focusedNode, 'focused') === true,
    `${expected.label} dialog focus is not exposed in the accessibility tree`,
  );

  sendNativeInput(
    child,
    expected.initialFocus === expected.last ? 'tab' : 'shift-tab',
  );
  await delay(120);
  check(
    (await focusedButtonText(client)) ===
      (expected.initialFocus === expected.last
        ? expected.first
        : expected.last),
    `${expected.label} dialog did not wrap focus from its initial boundary`,
  );
  sendNativeInput(
    child,
    expected.initialFocus === expected.last ? 'shift-tab' : 'tab',
  );
  await delay(120);
  check(
    (await focusedButtonText(client)) === expected.initialFocus,
    `${expected.label} dialog did not wrap focus back to its initial action`,
  );
  sendNativeInput(child, 'escape');
  await waitForValue(
    client,
    `!document.querySelector('.dialog')`,
    `${expected.label} Escape dismissal`,
  );
  const restoredFocus = await client.evaluate(
    `(() => ({
      matches: document.activeElement?.matches(${JSON.stringify(expected.returnFocus)}) ?? false,
      tag: document.activeElement?.tagName ?? '',
      id: document.activeElement?.id ?? '',
      classes: Array.from(document.activeElement?.classList ?? []),
      text: document.activeElement?.textContent?.trim() ?? ''
    }))()`,
    `${expected.label} restored focus`,
  );
  check(
    restoredFocus.matches,
    `${expected.label} did not restore invoker focus: ${JSON.stringify(restoredFocus)}`,
  );
}

async function verifyDeleteDialog(client, child) {
  await client.evaluate(
    `document.querySelector('.server-summary')?.click()`,
    'open server management for delete dialog',
  );
  await waitForValue(
    client,
    `document.querySelector('.server-management')`,
    'server management delete dialog',
  );
  await client.evaluate(
    domAction(`
      const button = findButton('Delete Accessibility fixture');
      if (!(button instanceof HTMLButtonElement)) throw new Error('Missing delete action');
      button.focus();
    `),
    'focus delete dialog invoker',
  );
  await waitForValue(
    client,
    `document.activeElement?.getAttribute('aria-label') === 'Delete Accessibility fixture'`,
    'focused delete dialog invoker',
  );
  await client.evaluate(
    domAction(`clickButton('Delete Accessibility fixture');`),
    'open delete dialog',
  );
  await verifyDialogAccessibility(client, child, {
    description:
      'This removes the saved server profile. This action cannot be undone.',
    first: 'Cancel',
    initialFocus: 'Delete profile',
    label: 'Delete profile',
    last: 'Delete profile',
    returnFocus: '.delete-icon-button',
    title: 'Delete “Accessibility fixture”?',
  });
  await client.evaluate(
    domAction(`clickButton('Back to connection');`),
    'leave server management',
  );
}

async function verifyUnsafeBlockDialog(client, child) {
  await client.evaluate(
    domAction(`
      const disclosure = document.querySelector('.disclosure-button');
      if (!(disclosure instanceof HTMLButtonElement)) throw new Error('Missing Advanced disclosure');
      disclosure.click();
    `),
    'expand Advanced settings for accessibility',
  );
  await waitForValue(
    client,
    `document.querySelector('#advanced-content')`,
    'expanded Advanced settings for accessibility',
  );
  await client.evaluate(
    domAction(`
      const label = Array.from(document.querySelectorAll('.override-toggle')).find((candidate) => candidate.querySelector('strong')?.textContent.trim() === 'Override KCP block');
      const checkbox = label?.querySelector('input[type="checkbox"]');
      if (!(checkbox instanceof HTMLInputElement)) throw new Error('Missing KCP block override');
      checkbox.click();
    `),
    'enable KCP block override',
  );
  await waitForValue(
    client,
    `document.querySelector('#kcp-block') instanceof HTMLSelectElement && !document.querySelector('#kcp-block').disabled`,
    'enabled KCP block override',
  );
  await client.evaluate(
    domAction(`
      const select = document.querySelector('#kcp-block');
      if (!(select instanceof HTMLSelectElement)) throw new Error('Missing select #kcp-block');
      select.focus();
    `),
    'focus unsafe KCP block invoker',
  );
  await waitForValue(
    client,
    `document.activeElement?.id === 'kcp-block'`,
    'focused unsafe KCP block invoker',
  );
  await client.evaluate(
    domAction(`setSelect('#kcp-block', 'none');`),
    'select insecure KCP block',
  );
  await verifyDialogAccessibility(client, child, {
    description:
      'Traffic will not be encrypted or authenticated. The server must use this exact value; None and Null are not interchangeable.',
    first: 'Cancel',
    initialFocus: 'Cancel',
    label: 'Unsafe KCP block',
    last: 'Use insecure block',
    returnFocus: '#kcp-block',
    title: 'Use insecure “none” KCP block?',
  });
}

async function verifyClipboardRoundTrip(session) {
  const { client } = session;
  const origin = new URL(session.pageUrl).origin;
  const sentinel = `paqet-clipboard-${randomBytes(32).toString('hex')}`;
  await client.send('Browser.grantPermissions', {
    origin,
    permissions: ['clipboardReadWrite', 'clipboardSanitizedWrite'],
  });
  console.log(
    'APP-001D1 clipboard warning: this destructive check replaces prior clipboard content with a non-secret sentinel and does not retain or restore it',
  );
  try {
    const roundTrip = await client.evaluate(
      `(async () => {
        if (typeof navigator.clipboard?.readText !== 'function' || typeof navigator.clipboard?.writeText !== 'function') {
          throw new Error('Clipboard text APIs are unavailable');
        }
        await navigator.clipboard.writeText(${JSON.stringify(sentinel)});
        return await navigator.clipboard.readText() === ${JSON.stringify(sentinel)};
      })()`,
      'destructive clipboard round trip',
      { userGesture: true },
    );
    check(roundTrip, 'Clipboard sentinel did not round-trip');
  } finally {
    await client.send('Browser.resetPermissions');
  }
}

async function verifyAccessibilityWorkflows(destructiveClipboard) {
  check(process.platform === 'win32', 'WebView verification requires Windows');
  check(process.arch === 'x64', 'WebView verification requires Windows x64');
  await access(executable);
  const testRoot = await mkdtemp(
    path.join(os.tmpdir(), 'paqet-accessibility-'),
  );
  const appData = path.join(testRoot, 'app-data');
  const fixturePath = path.join(appData, 'network-interfaces.json');
  const sessions = new Set();
  const key = `paqet-app001d1-${randomBytes(32).toString('hex')}`;
  secretSentinels = [key];
  let interrupted;
  const terminateOnSignal = (signal) => {
    interrupted ??= signal;
    for (const session of sessions) {
      try {
        terminateOwnedChild(session);
      } catch {
        process.exitCode = 1;
      }
    }
  };
  process.once('SIGINT', terminateOnSignal);
  process.once('SIGTERM', terminateOnSignal);
  process.once('SIGBREAK', terminateOnSignal);
  let evidence;
  let failure;
  let teardownFailure;
  try {
    await mkdir(appData, { recursive: true });
    await writeFile(
      fixturePath,
      `${JSON.stringify(
        [
          {
            friendlyName: 'Accessibility Ethernet',
            interfaceName: 'accessibility0',
            guid: '\\Device\\NPF_{33333333-3333-4333-8333-333333333333}',
            localAddress: '192.0.2.30',
            gatewayAddress: '192.0.2.1',
            gatewayMac: '02:00:00:00:03:01',
          },
        ],
        null,
        2,
      )}\n`,
    );
    const session = await launchApplication(
      testRoot,
      appData,
      1,
      { PAQET_GUI_TEST_NETWORK_FIXTURE: '1' },
      sessions,
    );
    await verifyEmulatedPreferences(session.client, session.child);
    await clientResetMedia(session.client);
    await createProfile(session.client, {
      host: '192.0.2.90',
      key,
      name: 'Accessibility fixture',
      port: 4901,
    });
    await session.client.evaluate(
      domAction(`clickButton('Back to connection');`),
      'return to accessibility connection view',
    );
    await waitForValue(
      session.client,
      `document.querySelector('.server-summary') && !document.querySelector('.server-management')`,
      'accessibility connection view',
    );
    await verifyAccessibilityShell(session.client);
    await verifyDeleteDialog(session.client, session.child);
    await verifyUnsafeBlockDialog(session.client, session.child);
    if (destructiveClipboard) {
      await verifyClipboardRoundTrip(session);
    }
    evidence = {
      version: session.version.product,
    };
    await session.client.send('Accessibility.disable');
    await closeApplication(session);
    sessions.delete(session);
  } catch (error) {
    const appOutput = [...sessions]
      .map((session) => session.appOutput.trim())
      .filter(Boolean)
      .join('\n');
    failure = appOutput
      ? new Error(redact(`${error.message}\nApplication output:\n${appOutput}`))
      : sanitizedError(error);
  } finally {
    const teardownErrors = [];
    for (const session of sessions) {
      if (!session.client?.closed) {
        try {
          await clientResetMedia(session.client);
          await session.client?.send('Accessibility.disable');
        } catch (error) {
          teardownErrors.push(sanitizedError(error));
        }
      }
      try {
        await closeApplication(session);
      } catch (error) {
        teardownErrors.push(sanitizedError(error));
      }
    }
    try {
      await rm(testRoot, {
        recursive: true,
        force: true,
        maxRetries: 10,
        retryDelay: 200,
      });
    } catch (error) {
      teardownErrors.push(sanitizedError(error));
    }
    process.off('SIGINT', terminateOnSignal);
    process.off('SIGTERM', terminateOnSignal);
    process.off('SIGBREAK', terminateOnSignal);
    if (teardownErrors.length > 0) {
      teardownFailure = new AggregateError(
        teardownErrors,
        'Accessibility WebView teardown failed',
      );
    }
  }
  if (failure && teardownFailure) {
    throw new AggregateError(
      [failure, teardownFailure],
      'Accessibility verification and teardown failed',
    );
  }
  if (failure) throw failure;
  if (teardownFailure) throw teardownFailure;
  if (interrupted)
    throw new Error(`Accessibility verification interrupted by ${interrupted}`);

  console.log(
    `Host: ${os.version()} ${os.arch()}, WebView2 ${evidence.version}`,
  );
  console.log(
    'APP-001D1 preferences: forced-colors and reduced-motion media queries activated; representative surface boundaries, keyboard focus, no horizontal overflow, and disclosure transition override verified',
  );
  console.log(
    'APP-001D1 accessibility: canonical shell roles plus primary-first and cancel-first modal semantics, focus wrapping, Escape dismissal, and invoker restoration verified',
  );
  if (destructiveClipboard) {
    console.log(
      'APP-001D1 clipboard: destructive non-secret text sentinel write/read round-trip verified; prior clipboard content was intentionally not retained or restored',
    );
  }
  console.log(
    destructiveClipboard
      ? 'Teardown: identity-recorded process tree exited with zero orphans; temporary clipboard permissions reset and temporary root deleted'
      : 'Teardown: identity-recorded process tree exited with zero orphans; media/accessibility emulation reset and temporary root deleted',
  );
}

async function clientResetMedia(client) {
  if (!client) return;
  await client.send('Emulation.setEmulatedMedia', { media: '', features: [] });
}

async function readInterfaceDetails(client) {
  return client.evaluate(
    `(() => ({
      optionCount: document.querySelector('#interface-select')?.options.length ?? 0,
      selectedGuid: document.querySelector('#interface-select')?.value ?? '',
      details: Object.fromEntries(Array.from(document.querySelectorAll('[aria-label="Derived interface details"] > div')).map((row) => [row.querySelector('dt')?.textContent.trim(), row.querySelector('dd')?.textContent.trim()]))
    }))()`,
    'interface details',
  );
}

function assertInterfaceDetails(actual, expected) {
  check(actual.optionCount === 2, 'Expected exactly two network interfaces');
  check(
    actual.selectedGuid === expected.guid,
    'Selected interface GUID changed',
  );
  check(
    actual.details['Interface name'] === expected.interfaceName &&
      actual.details['Npcap device'] === expected.guid &&
      actual.details['Local address'] === expected.localAddress &&
      actual.details['Gateway address'] === expected.gatewayAddress &&
      actual.details['Gateway MAC'] === expected.gatewayMac,
    'Rendered derived interface details do not match the fixture',
  );
}

async function listRegularFiles(directory) {
  const files = [];
  const pending = [directory];
  while (pending.length > 0) {
    const current = pending.pop();
    const entries = await readdir(current, { withFileTypes: true });
    for (const entry of entries) {
      const entryPath = path.join(current, entry.name);
      if (entry.isDirectory()) pending.push(entryPath);
      else if (entry.isFile()) files.push(entryPath);
      else if (entry.isSymbolicLink()) {
        throw new Error('Secret audit encountered an unexpected symbolic link');
      }
    }
  }
  return files;
}

async function auditSecrets(testRoot) {
  const foundPaths = new Set();
  const foundSentinels = new Set();
  for (const filePath of await listRegularFiles(testRoot)) {
    const contents = await readFile(filePath);
    const relativePath = path
      .relative(testRoot, filePath)
      .split(path.sep)
      .join('/');
    for (const sentinel of secretSentinels) {
      const representations = fileSecretRepresentations(sentinel);
      const percent = representations.pop().toString().toLowerCase();
      if (
        !representations.some((value) => contents.includes(value)) &&
        !contents.toString('latin1').toLowerCase().includes(percent)
      )
        continue;
      foundPaths.add(relativePath);
      foundSentinels.add(sentinel);
      check(
        permittedSecretPaths.has(relativePath),
        `Secret sentinel escaped to ${relativePath}`,
      );
    }
  }
  check(
    foundSentinels.size === secretSentinels.length,
    'Not every secret sentinel was found in an expected disclosed store',
  );
  return [...foundPaths].sort();
}

function sha256(contents) {
  return createHash('sha256').update(contents).digest('hex');
}

async function readVerifiedSidecar() {
  const contents = await readFile(copiedSidecar);
  check(
    contents.length === pinnedSidecarSize &&
      sha256(contents) === pinnedSidecarSha256,
    'The copied paqet sidecar does not match the pinned manifest identity',
  );
  return contents;
}

async function verifySidecarWorkflows() {
  check(process.platform === 'win32', 'Sidecar verification requires Windows');
  check(process.arch === 'x64', 'Sidecar verification requires Windows x64');
  await access(executable);
  const exactSidecar = await readVerifiedSidecar();
  secretSentinels = [`paqet-app001c-${randomBytes(32).toString('hex')}`];
  const testRoot = await mkdtemp(path.join(os.tmpdir(), 'paqet-sidecar-'));
  const appData = path.join(testRoot, 'app-data');
  const profile = {
    name: 'Pinned lifecycle',
    host: '192.0.2.80',
    port: 9999,
    key: secretSentinels[0],
  };
  const socksPort = await availableLoopbackPort();
  const sessions = new Set();
  const sidecarIdentities = [];
  let interrupted;
  let copiedSidecarRestored = true;
  let permittedPaths = [];
  let webViewVersion;
  let failure;
  let teardownFailure;
  const terminateOnSignal = (signal) => {
    interrupted ??= signal;
    for (const session of sessions) {
      try {
        terminateOwnedChild(session);
      } catch {
        process.exitCode = 1;
      }
    }
  };
  process.once('SIGINT', terminateOnSignal);
  process.once('SIGTERM', terminateOnSignal);
  process.once('SIGBREAK', terminateOnSignal);

  try {
    await mkdir(appData, { recursive: true });

    const corruptSidecar = wrongIdentitySidecar(exactSidecar);
    copiedSidecarRestored = false;
    await writeFile(copiedSidecar, corruptSidecar);
    check(
      corruptSidecar.length === pinnedSidecarSize &&
        sha256(corruptSidecar) !== pinnedSidecarSha256,
      'The wrong-identity sidecar fixture is not precise',
    );
    verifyWrongIdentityRemainsLaunchable();

    const rejection = await launchApplication(
      testRoot,
      appData,
      'sidecar-rejection',
      {},
      sessions,
    );
    webViewVersion = rejection.version.product;
    await createProfile(rejection.client, profile);
    await rejection.client.evaluate(
      domAction(`clickButton('Back to connection');`),
      'return from rejected sidecar server setup',
    );
    await waitForValue(
      rejection.client,
      `document.querySelector('#socks-port') && !document.querySelector('.server-management')`,
      'rejected sidecar connection view',
    );
    await rejection.client.evaluate(
      domAction(`
        const input = setInput('#socks-port', ${JSON.stringify(String(socksPort))});
        input.dispatchEvent(new Event('change', { bubbles: true }));
        clickButton('Connect');
      `),
      'connect with wrong sidecar identity',
    );
    await waitForValue(
      rejection.client,
      `(() => {
        const button = document.querySelector('.connect-button');
        const message = Array.from(document.querySelectorAll('.connection .app-message')).find((item) => item.textContent.trim() === 'The paqet client could not be started.');
        return Boolean(message) &&
          document.querySelector('.status')?.textContent.trim() === 'Disconnected' &&
          button?.textContent.trim() === 'Connect' && !button.disabled;
      })()`,
      'wrong sidecar identity rejection',
    );
    check(
      !processTreeIdentities(rejection.child.pid).some(
        ({ name, pid }) =>
          pid !== rejection.child.pid &&
          name.toLowerCase() === 'paqet_windows_amd64.exe',
      ),
      'A wrong-identity paqet process was launched',
    );
    await closeApplication(rejection);
    sessions.delete(rejection);

    await writeFile(copiedSidecar, exactSidecar);
    await readVerifiedSidecar();
    copiedSidecarRestored = true;

    const lifecycle = await launchApplication(
      testRoot,
      appData,
      'sidecar-lifecycle',
      { PAQET_GUI_TEST_ALLOW_PERSISTED_PROFILES: '1' },
      sessions,
    );
    check(
      lifecycle.version.product === webViewVersion,
      'WebView2 version changed between sidecar launches',
    );
    const { client } = lifecycle;

    await client.evaluate(
      domAction(`clickButton('Connect');`),
      'initial connect',
    );
    const firstPaqet = await waitForPaqetProcess(
      lifecycle,
      'the initial pinned paqet process',
    );
    sidecarIdentities.push(firstPaqet);
    await waitForValue(
      client,
      `(() => {
        const log = document.querySelector('[aria-label="Connection logs"]');
        return document.querySelector('.status')?.textContent.trim() === 'Connected' &&
          document.querySelector('.connect-button')?.textContent.trim() === 'Disconnect' &&
          log?.textContent.includes('Client started:') &&
          document.querySelector('#socks-port')?.value === ${JSON.stringify(String(socksPort))} &&
          log?.textContent.includes(${JSON.stringify(`SOCKS5 server listening on 127.0.0.1:${socksPort}`)});
      })()`,
      'initial real paqet connection',
    );
    const firstPaqetTree = processTreeIdentities(firstPaqet.pid);
    lifecycle.identities.push(...firstPaqetTree);
    sidecarIdentities.push(...firstPaqetTree);

    await client.evaluate(
      domAction(`clickButton('Disconnect');`),
      'disconnect',
    );
    await waitForValue(
      client,
      `(() => {
        const button = document.querySelector('.connect-button');
        return document.querySelector('.status')?.textContent.trim() === 'Disconnected' &&
          button?.textContent.trim() === 'Connect' && !button.disabled;
      })()`,
      'requested disconnect completion',
    );
    await waitForTestProcesses(firstPaqetTree, 5_000);

    await client.evaluate(
      domAction(`clickButton('Connect');`),
      'connect before unexpected exit',
    );
    const secondPaqet = await waitForPaqetProcess(
      lifecycle,
      'the second pinned paqet process',
    );
    sidecarIdentities.push(secondPaqet);
    check(
      secondPaqet.pid !== firstPaqet.pid ||
        secondPaqet.created !== firstPaqet.created,
      'Reconnect reused the initial process identity',
    );
    await waitForValue(
      client,
      `document.querySelector('.status')?.textContent.trim() === 'Connected'`,
      'connected state before unexpected exit',
    );
    const secondPaqetTree = processTreeIdentities(secondPaqet.pid);
    lifecycle.identities.push(...secondPaqetTree);
    sidecarIdentities.push(...secondPaqetTree);
    terminateExactProcess(secondPaqet);
    await waitForValue(
      client,
      `(() => {
        const failure = document.querySelector('.failure-message');
        const button = document.querySelector('.connect-button');
        return document.querySelector('.status')?.textContent.trim() === 'Disconnected' &&
          failure?.textContent.includes('exited unexpectedly') &&
          button?.textContent.trim() === 'Connect' && !button.disabled;
      })()`,
      'unexpected paqet exit recovery',
    );
    await waitForTestProcesses(secondPaqetTree, 5_000);

    await client.evaluate(
      domAction(`clickButton('Connect');`),
      'final reconnect',
    );
    const thirdPaqet = await waitForPaqetProcess(
      lifecycle,
      'the third pinned paqet process',
    );
    sidecarIdentities.push(thirdPaqet);
    check(
      sidecarIdentities
        .slice(0, -1)
        .every(
          (identity) =>
            identity.pid !== thirdPaqet.pid ||
            identity.created !== thirdPaqet.created,
        ),
      'Final reconnect reused a prior process identity',
    );
    await waitForValue(
      client,
      `document.querySelector('.status')?.textContent.trim() === 'Connected'`,
      'connected state before supervised close',
    );
    await closeActiveApplication(lifecycle, thirdPaqet);
    sessions.delete(lifecycle);
    await waitForTestProcesses(sidecarIdentities, 5_000);
    permittedPaths = await auditSecrets(testRoot);
  } catch (error) {
    const appOutput = [...sessions]
      .map((session) => session.appOutput.trim())
      .filter(Boolean)
      .join('\n');
    failure = appOutput
      ? new Error(redact(`${error.message}\nApplication output:\n${appOutput}`))
      : sanitizedError(error);
  } finally {
    const teardownErrors = [];
    for (const session of sessions) {
      try {
        await forceCloseApplication(session);
      } catch (error) {
        teardownErrors.push(sanitizedError(error));
      }
    }
    try {
      await waitForTestProcesses(sidecarIdentities, 5_000);
    } catch (error) {
      teardownErrors.push(sanitizedError(error));
    }
    if (!copiedSidecarRestored) {
      try {
        await writeFile(copiedSidecar, exactSidecar);
        await readVerifiedSidecar();
        copiedSidecarRestored = true;
      } catch (error) {
        teardownErrors.push(sanitizedError(error));
      }
    }
    try {
      await readVerifiedSidecar();
    } catch (error) {
      teardownErrors.push(sanitizedError(error));
    }
    try {
      await rm(testRoot, {
        recursive: true,
        force: true,
        maxRetries: 10,
        retryDelay: 200,
      });
    } catch (error) {
      teardownErrors.push(sanitizedError(error));
    }
    process.off('SIGINT', terminateOnSignal);
    process.off('SIGTERM', terminateOnSignal);
    process.off('SIGBREAK', terminateOnSignal);
    if (teardownErrors.length > 0) {
      teardownFailure = new AggregateError(
        teardownErrors,
        'Sidecar WebView teardown failed',
      );
    }
  }
  if (failure && teardownFailure) {
    throw new AggregateError(
      [failure, teardownFailure],
      'Sidecar verification and teardown failed',
    );
  }
  if (failure) throw failure;
  if (teardownFailure) throw teardownFailure;
  if (interrupted)
    throw new Error(`Sidecar verification interrupted by ${interrupted}`);

  console.log(`Host: ${os.version()} ${os.arch()}, WebView2 ${webViewVersion}`);
  console.log(
    `APP-001C identity: wrong copied SHA-256 rejected before spawn; restored sidecar verified at ${pinnedSidecarSize} bytes / ${pinnedSidecarSha256}`,
  );
  console.log(
    'APP-001C lifecycle: persisted custom loopback SOCKS port, real pinned paqet startup marker/listener, requested disconnect, unexpected exit recovery, and reconnect verified',
  );
  console.log(
    `APP-001C close/cleanup: supervised active close confirmed; ${uniqueIdentities(sidecarIdentities).length} recorded sidecar-tree identities exited; sentinels restricted to ${permittedPaths.join(', ')}; temporary root deleted`,
  );
}

async function verifyDisconnectedWorkflows() {
  check(process.platform === 'win32', 'WebView verification requires Windows');
  check(process.arch === 'x64', 'WebView verification requires Windows x64');
  await access(executable);
  secretSentinels = Array.from(
    { length: 3 },
    () => `paqet-app001b-${randomBytes(32).toString('hex')}`,
  );
  const testRoot = await mkdtemp(path.join(os.tmpdir(), 'paqet-webview-'));
  const appData = path.join(testRoot, 'app-data');
  const fixturePath = path.join(appData, 'network-interfaces.json');
  const configPath = path.join(appData, 'local', 'config.yaml');
  const settingsPath = path.join(appData, 'config', 'settings.json');
  const releaseGate = path.join(appData, 'release-launch-failure');
  const interfaces = [
    {
      friendlyName: 'Fixture Ethernet',
      interfaceName: 'fixture0',
      guid: '\\Device\\NPF_{11111111-1111-4111-8111-111111111111}',
      localAddress: '192.0.2.10',
      gatewayAddress: '192.0.2.1',
      gatewayMac: '02:00:00:00:01:01',
    },
    {
      friendlyName: 'Fixture Wi-Fi',
      interfaceName: 'fixture1',
      guid: '\\Device\\NPF_{22222222-2222-4222-8222-222222222222}',
      localAddress: '198.51.100.20',
      gatewayAddress: '198.51.100.1',
      gatewayMac: '02:00:00:00:02:01',
    },
  ];
  const refreshedSecondInterface = {
    ...interfaces[1],
    localAddress: '203.0.113.21',
    gatewayAddress: '203.0.113.1',
    gatewayMac: '02:00:00:00:02:99',
  };
  const refreshedInterfaces = [interfaces[0], refreshedSecondInterface];
  const socksPort = 20_080;
  const primary = {
    name: 'Primary',
    host: '192.0.2.80',
    port: 4101,
    key: secretSentinels[0],
  };
  const backup = {
    name: 'Backup',
    host: '198.51.100.80',
    port: 4102,
    key: secretSentinels[1],
  };
  const backupUpdated = {
    name: 'Backup updated',
    host: '203.0.113.80',
    port: 5102,
    key: secretSentinels[2],
  };
  const temporary = {
    name: 'Temporary',
    host: '192.0.2.81',
    port: 4103,
    key: `${secretSentinels[1]}-temporary`,
  };
  const sessions = new Set();
  let interrupted;
  const terminateOnSignal = (signal) => {
    interrupted ??= signal;
    for (const session of sessions) {
      try {
        terminateOwnedChild(session);
      } catch {
        process.exitCode = 1;
      }
    }
  };
  process.once('SIGINT', terminateOnSignal);
  process.once('SIGTERM', terminateOnSignal);
  process.once('SIGBREAK', terminateOnSignal);
  let app001aEvidence;
  let serverCardZoom;
  let webViewVersion;
  let permittedPaths = [];
  let failure;
  let teardownFailure;
  try {
    await mkdir(appData, { recursive: true });
    await writeFile(fixturePath, `${JSON.stringify(interfaces, null, 2)}\n`);

    const launch1 = await launchApplication(
      testRoot,
      appData,
      1,
      {
        PAQET_GUI_TEST_LAUNCH_MODE: 'delayed-failure',
        PAQET_GUI_TEST_NETWORK_FIXTURE: '1',
      },
      sessions,
    );
    webViewVersion = launch1.version.product;
    app001aEvidence = await verifyApp001A(launch1);

    const { client: client1 } = launch1;
    await createProfile(client1, primary);
    await createProfile(client1, backup);
    await selectProfileByName(client1, backup.name);
    await editSelectedProfile(client1, backupUpdated);
    await createProfile(client1, temporary);
    await selectProfileByName(client1, temporary.name);
    await deleteSelectedProfile(client1, temporary.name);
    const finalLaunch1Profiles = await client1.evaluate(
      profileViewExpression(),
      'launch 1 final profiles',
    );
    check(
      JSON.stringify(finalLaunch1Profiles.names) ===
        JSON.stringify([primary.name, backupUpdated.name]) &&
        finalLaunch1Profiles.selectedName === backupUpdated.name,
      'Launch 1 did not end with exactly Primary and selected Backup updated',
    );
    await client1.evaluate(
      domAction(
        `clickButton(${JSON.stringify(`Edit ${backupUpdated.name}`)});`,
      ),
      'open editor for dirty close evidence',
    );
    await waitForValue(
      client1,
      `document.querySelector('#profile-name') && document.activeElement?.id === 'profile-name'`,
      'editor for dirty close evidence',
    );
    await client1.evaluate(
      domAction(`
        const input = setInput('#profile-name', 'Unsaved close check');
        input.focus();
      `),
      'prepare dirty editor close evidence',
    );
    await waitForValue(
      client1,
      `document.querySelector('#profile-name')?.value === 'Unsaved close check'`,
      'dirty editor close draft',
    );
    sendNativeInput(launch1.child, 'close');
    await waitForValue(
      client1,
      `document.querySelector('.dialog h2')?.textContent.trim() === 'Discard changes and close?'`,
      'dirty editor window-close confirmation',
    );
    await client1.evaluate(
      domAction(`clickButton('Keep editing');`),
      'cancel dirty editor window close',
    );
    await waitForValue(
      client1,
      `document.querySelector('#profile-name')?.value === 'Unsaved close check' && !document.querySelector('.dialog')`,
      'preserved draft after canceled window close',
    );
    await client1.evaluate(
      domAction(`clickButton('Cancel');`),
      'request close-check draft discard',
    );
    await waitForValue(
      client1,
      `document.querySelector('.dialog h2')?.textContent.trim() === 'Discard your changes?'`,
      'close-check draft discard dialog',
    );
    await client1.evaluate(
      domAction(`clickButton('Discard changes');`),
      'confirm close-check draft discard',
    );
    await waitForValue(
      client1,
      `!document.querySelector('#profile-name') && !document.querySelector('.dialog')`,
      'discarded close-check draft',
    );
    await client1.evaluate(
      domAction(`clickButton('Back to connection');`),
      'return to connection after server workflow',
    );
    await waitForValue(
      client1,
      `document.querySelector('.server-summary') && !document.querySelector('.server-management')`,
      'connection view after server workflow',
    );
    await client1.evaluate(
      `document.querySelector('.server-summary')?.click()`,
      'open servers for zoom evidence',
    );
    await waitForValue(
      client1,
      `document.querySelector('.server-management')`,
      'server view for zoom evidence',
    );
    sendNativeInput(launch1.child, 'zoom-in', 5);
    await delay(500);
    const serverZoom = await readMetrics(client1);
    assertNoHorizontalOverflow(serverZoom, 'Server management at 200% zoom');
    await assertReachable(
      client1,
      '.management-header',
      'Server management header',
    );
    const lastServerReachable = await client1.evaluate(
      `(async () => {
        const scroller = document.querySelector('.server-management');
        const action = document.querySelector('.server-card:last-of-type .server-card-select');
        if (!(scroller instanceof HTMLElement) || !(action instanceof HTMLElement)) return false;
        scroller.scrollTop = scroller.scrollHeight;
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const viewport = scroller.getBoundingClientRect();
        const bounds = action.getBoundingClientRect();
        return bounds.top >= viewport.top - 1 && bounds.bottom <= viewport.bottom + 1;
      })()`,
      'last saved server card nested reachability',
    );
    check(
      lastServerReachable,
      'Last saved server card action is not reachable in the server view',
    );
    const serverActionsVisible = await client1.evaluate(
      `(() => {
        const actions = document.querySelector('.server-card:last-of-type .server-card-actions');
        if (!(actions instanceof HTMLElement)) return false;
        const bounds = actions.getBoundingClientRect();
        return bounds.left >= -1 && bounds.right <= innerWidth + 1;
      })()`,
      'last saved server card action visibility',
    );
    check(
      serverActionsVisible,
      'Last saved server card actions are horizontally clipped',
    );
    serverCardZoom = await client1.evaluate(
      `(() => {
        const card = document.querySelector('.server-card:last-of-type');
        const select = card?.querySelector('.server-card-select');
        const copy = card?.querySelector('.server-card-copy');
        const name = copy?.querySelector('strong');
        const actions = card?.querySelector('.server-card-actions');
        const text = name?.firstChild;
        if (!(select instanceof HTMLElement) || !(copy instanceof HTMLElement) ||
            !(name instanceof HTMLElement) || !(actions instanceof HTMLElement) ||
            !(text instanceof Text)) return null;
        const charactersByLine = new Map();
        for (let index = 0; index < text.length; index += 1) {
          if (/\\s/.test(text.data[index] ?? '')) continue;
          const range = document.createRange();
          range.setStart(text, index);
          range.setEnd(text, index + 1);
          const top = Math.round(range.getBoundingClientRect().top);
          charactersByLine.set(top, (charactersByLine.get(top) ?? 0) + 1);
        }
        return {
          actionWidth: actions.getBoundingClientRect().width,
          copyWidth: copy.getBoundingClientRect().width,
          lineCount: charactersByLine.size,
          maxCharactersOnLine: Math.max(0, ...charactersByLine.values()),
          name: name.textContent.trim(),
          selectWidth: select.getBoundingClientRect().width
        };
      })()`,
      'selected server card text layout at 200% zoom',
    );
    check(serverCardZoom, 'Selected server card layout could not be measured');
    check(
      serverCardZoom.copyWidth >= 72 &&
        serverCardZoom.selectWidth >= 150 &&
        serverCardZoom.actionWidth >= 150 &&
        serverCardZoom.maxCharactersOnLine >= 3 &&
        serverCardZoom.lineCount <=
          Math.ceil(serverCardZoom.name.replace(/\\s/g, '').length / 2),
      `Selected server name collapsed at 200% zoom: ${JSON.stringify(serverCardZoom)}`,
    );
    await client1.evaluate(
      domAction(`
        const selectedName = document.querySelector('.selected-server strong')?.textContent.trim();
        clickButton('Edit ' + selectedName);
      `),
      'open selected server editor at 200% zoom',
    );
    await waitForValue(
      client1,
      `document.querySelector('.server-editor') && document.activeElement?.id === 'profile-name'`,
      'server editor at 200% zoom',
    );
    assertNoHorizontalOverflow(
      await readMetrics(client1),
      'Server editor at 200% zoom',
    );
    await assertReachable(
      client1,
      '.server-editor .form-actions',
      'Server editor actions',
    );
    await client1.evaluate(
      domAction(`clickButton('Cancel');`),
      'close zoomed server editor',
    );
    retryNativeInput(launch1.child, 'zoom-reset');
    await delay(300);
    await client1.evaluate(
      domAction(`clickButton('Back to connection');`),
      'return after server zoom evidence',
    );

    await client1.evaluate(
      domAction(`
        const disclosure = document.querySelector('.disclosure-button');
        if (!(disclosure instanceof HTMLButtonElement)) throw new Error('Missing Advanced disclosure');
        disclosure.click();
      `),
      'expand Advanced settings',
    );
    await waitForValue(
      client1,
      `document.querySelector('.disclosure-button')?.getAttribute('aria-expanded') === 'true' && document.querySelector('#advanced-content')`,
      'expanded Advanced settings',
    );
    const initialInterface = await readInterfaceDetails(client1);
    check(
      initialInterface.optionCount === 2,
      'Expected two fixture interfaces',
    );
    await client1.evaluate(
      domAction(
        `setSelect('#interface-select', ${JSON.stringify(interfaces[1].guid)});`,
      ),
      'select second interface',
    );
    await waitForValue(
      client1,
      `document.querySelector('#interface-select')?.value === ${JSON.stringify(interfaces[1].guid)} && !document.querySelector('.interface-progress')`,
      'second interface selection',
    );
    assertInterfaceDetails(await readInterfaceDetails(client1), interfaces[1]);

    await writeFile(
      fixturePath,
      `${JSON.stringify(refreshedInterfaces, null, 2)}\n`,
    );
    await client1.evaluate(
      domAction(`clickButton('Refresh');`),
      'refresh interfaces',
    );
    await waitForValue(
      client1,
      `(() => {
        const rows = Array.from(document.querySelectorAll('[aria-label="Derived interface details"] > div'));
        const details = Object.fromEntries(rows.map((row) => [row.querySelector('dt')?.textContent.trim(), row.querySelector('dd')?.textContent.trim()]));
        return document.querySelector('#interface-select')?.value === ${JSON.stringify(refreshedSecondInterface.guid)} &&
          details['Local address'] === ${JSON.stringify(refreshedSecondInterface.localAddress)} &&
          details['Gateway MAC'] === ${JSON.stringify(refreshedSecondInterface.gatewayMac)} &&
          !Array.from(document.querySelectorAll('button')).some((button) => button.textContent.trim() === 'Refreshing…');
      })()`,
      'refreshed canonical interface details',
    );
    assertInterfaceDetails(
      await readInterfaceDetails(client1),
      refreshedSecondInterface,
    );

    await client1.evaluate(
      domAction(`
        const label = Array.from(document.querySelectorAll('.override-toggle')).find((candidate) => candidate.querySelector('strong')?.textContent.trim() === 'Override connection count');
        const checkbox = label?.querySelector('input[type="checkbox"]');
        if (!(checkbox instanceof HTMLInputElement)) throw new Error('Missing connection count override');
        checkbox.click();
      `),
      'enable connection count override',
    );
    await waitForValue(
      client1,
      `(() => {
        const input = document.querySelector('#connection-count');
        const label = input?.closest('.override-item')?.querySelector('.override-toggle input');
        return label?.checked && input instanceof HTMLInputElement && !input.disabled;
      })()`,
      'enabled connection count override',
    );
    await client1.evaluate(
      domAction(`
        const input = setInput('#connection-count', '3');
        input.dispatchEvent(new Event('blur'));
      `),
      'commit connection count override',
    );
    await waitForValue(
      client1,
      `(() => {
        const input = document.querySelector('#connection-count');
        return input?.value === '3' && !input.disabled && !document.querySelector('.settings-progress') && !document.querySelector('#connection-count-error');
      })()`,
      'canonical connection count',
    );

    const defaultSocksPort = await client1.evaluate(
      `document.querySelector('#socks-port')?.value`,
      'default SOCKS port',
    );
    check(
      defaultSocksPort === '1080',
      'Fresh SOCKS port did not default to 1080',
    );
    await client1.evaluate(
      domAction(`
        const input = setInput('#socks-port', '0');
        input.dispatchEvent(new Event('change', { bubbles: true }));
      `),
      'invalid SOCKS port draft',
    );
    await waitForValue(
      client1,
      `document.querySelector('#socks-port')?.getAttribute('aria-invalid') === 'true' && document.querySelector('#socks-port')?.value === '0'`,
      'invalid SOCKS port validation',
    );
    sendNativeInput(launch1.child, 'zoom-in', 5);
    await delay(500);
    const invalidZoom = await readMetrics(client1);
    assertNoHorizontalOverflow(invalidZoom, 'Invalid SOCKS port at 200% zoom');
    await assertReachable(client1, '#socks-port-error', 'SOCKS port error');
    await assertReachable(
      client1,
      '.connect-button',
      'Primary connection action',
    );
    await assertReachable(
      client1,
      '[aria-label="Connection logs"]',
      'Log surface',
    );
    sendNativeInput(launch1.child, 'zoom-reset');
    await delay(300);
    await client1.evaluate(
      domAction(`
        const input = setInput('#socks-port', ${JSON.stringify(String(socksPort))});
        input.dispatchEvent(new Event('change', { bubbles: true }));
        clickButton('Connect');
      `),
      'commit SOCKS port and connect',
    );

    const connecting = await waitForValue(
      client1,
      `(() => {
        const button = document.querySelector('.connect-button');
        return document.querySelector('.status')?.textContent.trim() === 'Connecting' &&
          button?.textContent.trim() === 'Connecting…' && button.disabled &&
          document.querySelector('#socks-port')?.disabled && document.querySelector('#socks-port')?.value === ${JSON.stringify(String(socksPort))} &&
          !document.querySelector('.server-summary')?.disabled &&
          document.querySelector('#interface-select')?.disabled &&
          document.querySelector('#connection-count')?.disabled;
      })()`,
      'canonical Connecting lock state',
    );
    check(connecting === true, 'Configuration controls were not locked');
    await client1.evaluate(
      `document.querySelector('.server-summary')?.click()`,
      'inspect servers while Connecting',
    );
    const connectingServerLocks = await waitForValue(
      client1,
      `(() => {
        const management = document.querySelector('.server-management');
        const lockedActions = Array.from(management?.querySelectorAll('.add-server-button, .server-card-select, .server-card-actions button') ?? []);
        return Boolean(management?.querySelector('.management-lock')) &&
          lockedActions.length >= 4 && lockedActions.every((control) => control.disabled) &&
          !management.querySelector('.back-button')?.disabled;
      })()`,
      'Connecting server management locks',
    );
    check(
      connectingServerLocks,
      'Server management mutations were not locked while Connecting',
    );
    await client1.evaluate(
      domAction(`clickButton('Back to connection');`),
      'return from Connecting server inspection',
    );

    await waitForPath(configPath, launch1.child, 'generated config.yaml');
    await waitForPath(settingsPath, launch1.child, 'persisted settings.json');
    const generatedConfig = await readFile(configPath, 'utf8');
    check(
      generatedConfig.includes(
        `addr: ${backupUpdated.host}:${backupUpdated.port}`,
      ) &&
        generatedConfig.includes(`key: ${backupUpdated.key}`) &&
        generatedConfig.includes(
          `interface: ${refreshedSecondInterface.interfaceName}`,
        ) &&
        generatedConfig.includes(`guid: ${refreshedSecondInterface.guid}`) &&
        generatedConfig.includes(
          `addr: ${refreshedSecondInterface.localAddress}:0`,
        ) &&
        generatedConfig.includes(
          `router_mac: ${refreshedSecondInterface.gatewayMac}`,
        ) &&
        generatedConfig.includes(`listen: 127.0.0.1:${socksPort}`) &&
        generatedConfig.includes('conn: 3'),
      'Generated configuration does not contain the selected canonical values',
    );
    check(
      !generatedConfig.includes(primary.key) &&
        !generatedConfig.includes(backup.key) &&
        !generatedConfig.includes(temporary.key),
      'Generated configuration contains an unselected or deleted secret',
    );
    await writeFile(releaseGate, 'release\n');
    await waitForValue(
      client1,
      `(() => {
        const button = document.querySelector('.connect-button');
        const status = document.querySelector('.status');
        const message = Array.from(document.querySelectorAll('.connection .app-message')).find((item) => item.textContent.trim() === 'The paqet client could not be started.');
        const log = document.querySelector('[aria-label="Connection logs"]');
        return Boolean(message) && status?.textContent.trim() === 'Disconnected' &&
          button?.textContent.trim() === 'Connect' && !button.disabled &&
          !document.querySelector('#interface-select')?.disabled &&
          !document.querySelector('#socks-port')?.disabled && document.querySelector('#socks-port')?.value === ${JSON.stringify(String(socksPort))} &&
          !document.querySelector('#connection-count')?.disabled &&
          log?.querySelectorAll('.log-record, .log-gap').length === 0 &&
          log?.textContent.trim() === 'Connection output will appear here.';
      })()`,
      'exact launch failure recovery',
    );

    await closeApplication(launch1);
    sessions.delete(launch1);

    const launch2 = await launchApplication(
      testRoot,
      appData,
      2,
      {
        PAQET_GUI_TEST_ALLOW_PERSISTED_PROFILES: '1',
        PAQET_GUI_TEST_NETWORK_FIXTURE: '1',
      },
      sessions,
    );
    check(
      launch2.version.product === webViewVersion,
      'WebView2 version changed between launches',
    );
    const { client: client2 } = launch2;
    const launch2Profiles = await client2.evaluate(
      profileViewExpression(),
      'restart persisted profiles',
    );
    check(
      JSON.stringify(launch2Profiles.names) === JSON.stringify([]) &&
        launch2Profiles.selectedName === backupUpdated.name &&
        !launch2Profiles.keyOnMain,
      'Restart did not restore the selected updated server summary safely',
    );
    await client2.evaluate(
      `document.querySelector('.server-summary')?.click()`,
      'open restart servers',
    );
    await waitForValue(
      client2,
      `document.querySelector('.server-management')`,
      'restart server management',
    );
    await client2.evaluate(
      domAction(
        `clickButton(${JSON.stringify(`Edit ${backupUpdated.name}`)});`,
      ),
      'open restart server editor',
    );
    const restartKeyMatches = await client2.evaluate(
      `document.querySelector('#encryption-key')?.type === 'password' && document.querySelector('#encryption-key')?.value === ${JSON.stringify(backupUpdated.key)}`,
      'restart masked encryption key verification',
    );
    check(
      restartKeyMatches,
      'Restart did not restore the updated encryption key in masked editor',
    );
    await client2.evaluate(
      domAction(`clickButton('Cancel'); clickButton('Back to connection');`),
      'leave restart server editor',
    );

    await client2.evaluate(
      domAction(`
        const disclosure = document.querySelector('.disclosure-button');
        if (!(disclosure instanceof HTMLButtonElement)) throw new Error('Missing Advanced disclosure');
        disclosure.click();
      `),
      'expand Advanced settings after restart',
    );
    await waitForValue(
      client2,
      `document.querySelector('#advanced-content')`,
      'restart Advanced settings',
    );
    const restartInterface = await readInterfaceDetails(client2);
    assertInterfaceDetails(restartInterface, refreshedInterfaces[0]);
    const restartState = await client2.evaluate(
      `(() => {
        const countInput = document.querySelector('#connection-count');
        const countToggle = countInput?.closest('.override-item')?.querySelector('.override-toggle input');
        const log = document.querySelector('[aria-label="Connection logs"]');
        return {
          status: document.querySelector('.status')?.textContent.trim(),
          connect: document.querySelector('.connect-button')?.textContent.trim(),
          countChecked: countToggle?.checked,
          countDisabled: countInput?.disabled,
          countValue: countInput?.value,
          socksPort: document.querySelector('#socks-port')?.value,
          socksPortDisabled: document.querySelector('#socks-port')?.disabled,
          logRecords: log?.querySelectorAll('.log-record, .log-gap').length,
          logText: log?.textContent.trim()
        };
      })()`,
      'restart session-only state',
    );
    check(
      restartState.status === 'Disconnected' &&
        restartState.connect === 'Connect' &&
        restartState.countChecked === false &&
        restartState.countDisabled === true &&
        restartState.countValue === '1' &&
        restartState.socksPort === String(socksPort) &&
        restartState.socksPortDisabled === false &&
        restartState.logRecords === 0 &&
        restartState.logText === 'Connection output will appear here.',
      'Restart did not reset interface-independent session state',
    );

    await closeApplication(launch2);
    sessions.delete(launch2);
    permittedPaths = await auditSecrets(testRoot);
  } catch (error) {
    const appOutput = [...sessions]
      .map((session) => session.appOutput.trim())
      .filter(Boolean)
      .join('\n');
    if (appOutput) {
      failure = new Error(
        redact(`${error.message}\nApplication output:\n${appOutput}`),
      );
    } else {
      failure = sanitizedError(error);
    }
  } finally {
    const teardownErrors = [];
    for (const session of sessions) {
      try {
        await closeApplication(session);
      } catch (error) {
        teardownErrors.push(sanitizedError(error));
      }
    }
    try {
      await rm(testRoot, {
        recursive: true,
        force: true,
        maxRetries: 10,
        retryDelay: 200,
      });
    } catch (error) {
      teardownErrors.push(sanitizedError(error));
    }
    process.off('SIGINT', terminateOnSignal);
    process.off('SIGTERM', terminateOnSignal);
    process.off('SIGBREAK', terminateOnSignal);
    if (teardownErrors.length > 0) {
      teardownFailure = new AggregateError(
        teardownErrors,
        'WebView teardown failed',
      );
    }
  }
  if (failure && teardownFailure) {
    throw new AggregateError(
      [failure, teardownFailure],
      'WebView verification and teardown failed',
    );
  }
  if (failure) throw failure;
  if (teardownFailure) throw teardownFailure;
  if (interrupted)
    throw new Error(`WebView verification interrupted by ${interrupted}`);

  const { baseline, focused, nativeMetrics, zoomed, zoomRatio } =
    app001aEvidence;
  console.log(`Host: ${os.version()} ${os.arch()}, WebView2 ${webViewVersion}`);
  console.log(
    `APP-001A default: client ${baseline.innerWidth}x${baseline.innerHeight} CSS px / ${nativeMetrics.clientWidth}x${nativeMetrics.clientHeight} physical px, window ${nativeMetrics.windowWidth}x${nativeMetrics.windowHeight} physical px at ${nativeMetrics.dpi} DPI, DPR ${baseline.devicePixelRatio}, document ${baseline.scrollWidth}x${baseline.scrollHeight}`,
  );
  console.log(
    `APP-001A zoom: ${Math.round(zoomRatio * 100)}%, viewport ${zoomed.innerWidth}x${zoomed.innerHeight} CSS px, document ${zoomed.scrollWidth}x${zoomed.scrollHeight}, no horizontal overflow; primary and log surfaces reachable`,
  );
  console.log(
    `UI-005 server zoom: selected card copy/select/actions ${Math.round(serverCardZoom.copyWidth)}/${Math.round(serverCardZoom.selectWidth)}/${Math.round(serverCardZoom.actionWidth)} CSS px, ${serverCardZoom.lineCount} name lines with up to ${serverCardZoom.maxCharactersOnLine} non-space characters per line`,
  );
  console.log(`APP-001A keyboard: ${focused.join(' -> ')}`);
  console.log(
    'APP-001A clipboard/fonts: secure write API available (non-mutating); Space Grotesk interface and JetBrains Mono logs loaded',
  );
  console.log(
    'APP-001B launch 1: real profile create/select/edit/delete, persisted global SOCKS validation/commit, two-interface refresh with GUID preservation, connection-count override, Connecting locks, generated YAML, exact launch failure, editable recovery, and empty logs verified',
  );
  console.log(
    'APP-001B launch 2: persisted selected profiles and masked updated key restored; interface selection, Advanced override, lifecycle, and logs reset verified',
  );
  console.log(
    `Secret audit: exact sentinels restricted to ${permittedPaths.join(', ')}`,
  );
  console.log(
    'Teardown: both identity-recorded process trees exited with zero orphans; temporary root deleted',
  );
}

const flags = new Set(process.argv.slice(2));
const knownFlags = new Set([
  '--accessibility',
  '--destructive-clipboard',
  '--sidecar',
]);
for (const flag of flags) {
  check(knownFlags.has(flag), `Unknown WebView verification flag ${flag}`);
}
check(
  !(flags.has('--sidecar') && flags.size > 1),
  'Sidecar verification cannot be combined with another workflow',
);
check(
  !flags.has('--destructive-clipboard') || flags.has('--accessibility'),
  'Destructive clipboard verification requires --accessibility',
);

if (flags.has('--accessibility')) {
  await verifyAccessibilityWorkflows(flags.has('--destructive-clipboard'));
} else if (flags.has('--sidecar')) {
  await verifySidecarWorkflows();
} else {
  await verifyDisconnectedWorkflows();
}
