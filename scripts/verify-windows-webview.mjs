import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { access, mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import os from 'node:os';
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
const expectedWidth = 440;
const expectedHeight = 760;
const commandTimeoutMs = 10_000;
const launchTimeoutMs = 30_000;

function check(condition, message) {
  if (!condition) throw new Error(message);
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
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
          candidate.title !== 'paqet' ||
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
      for (const pending of this.pending.values()) {
        clearTimeout(pending.timeout);
        pending.reject(new Error('WebView2 DevTools connection closed'));
      }
      this.pending.clear();
    });
  }

  async send(method, params = {}) {
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

  async evaluate(expression, label = 'evaluation') {
    let result;
    try {
      result = await this.send('Runtime.evaluate', {
        expression,
        awaitPromise: true,
        returnByValue: true,
      });
    } catch (error) {
      throw new Error(`WebView ${label} failed: ${error.message}`, {
        cause: error,
      });
    }
    if (result.exceptionDetails) {
      throw new Error(
        result.exceptionDetails.exception?.description ??
          result.exceptionDetails.text ??
          'WebView evaluation failed',
      );
    }
    return result.result.value;
  }

  close() {
    this.socket.close();
  }
}

async function waitForApplication(client) {
  const deadline = Date.now() + launchTimeoutMs;
  while (Date.now() < deadline) {
    const ready = await client.evaluate(
      `(() => ({
      documentReady: document.readyState === 'complete',
      hasShell: Boolean(document.querySelector('.app-shell')),
      hasConnection: Boolean(document.querySelector('.connect-button')),
      hasLogs: Boolean(document.querySelector('[aria-label="Log actions"]')),
      loading: document.querySelector('.configuration')?.getAttribute('aria-busy') === 'true'
    }))()`,
      'startup readiness',
    );
    if (
      ready.documentReady &&
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
if (-not [PaqetInput]::SetForegroundWindow($handle)) { throw 'cannot focus paqet main window' }
Start-Sleep -Milliseconds 40
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
  } elseif ($mode -eq 'tab') {
    for ($index = 0; $index -lt $count; $index++) {
      if ([PaqetInput]::GetForegroundWindow() -ne $handle) { throw 'paqet lost foreground ownership' }
      [PaqetInput]::keybd_event(0x09, 0, 0, [UIntPtr]::Zero)
      [PaqetInput]::keybd_event(0x09, 0, $KEYUP, [UIntPtr]::Zero)
      Start-Sleep -Milliseconds 80
    }
  } else {
    throw "unknown input mode $mode"
  }
} finally {
  [PaqetInput]::keybd_event(0x11, 0, $KEYUP, [UIntPtr]::Zero)
  [PaqetInput]::keybd_event(0x12, 0, $KEYUP, [UIntPtr]::Zero)
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
        `Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class KeyRelease { [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra); }'; 0x09,0x11,0x12,0x30,0xBB | ForEach-Object { [KeyRelease]::keybd_event($_, 0, 2, [UIntPtr]::Zero) }`,
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
      `Windows input ${mode} failed: ${(result.stderr || result.stdout).trim()}`,
    );
  }
  return result.stdout.trim();
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
    focused.length === 3 &&
      focused[0].includes('.compact-button.secondary-button') &&
      focused[1].includes('.secondary-button') &&
      focused[2].includes('.disclosure-button'),
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

async function main() {
  check(process.platform === 'win32', 'WebView verification requires Windows');
  check(process.arch === 'x64', 'WebView verification requires Windows x64');
  await access(executable);
  const testRoot = await mkdtemp(path.join(os.tmpdir(), 'paqet-webview-'));
  const userData = path.join(testRoot, 'webview');
  const appData = path.join(testRoot, 'app-data');
  const child = spawn(executable, [], {
    cwd: root,
    env: {
      ...process.env,
      PAQET_GUI_TEST_DATA_DIR: appData,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:
        '--remote-debugging-address=127.0.0.1 --remote-debugging-port=0',
      WEBVIEW2_USER_DATA_FOLDER: userData,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let hostIdentity;
  let interrupted;
  const terminateOnSignal = (signal) => {
    interrupted ??= signal;
    try {
      terminateProcessTree(hostIdentity);
    } catch {
      process.exitCode = 1;
    }
  };
  process.once('SIGINT', terminateOnSignal);
  process.once('SIGTERM', terminateOnSignal);
  process.once('SIGBREAK', terminateOnSignal);
  let appOutput = '';
  child.stdout.on('data', (chunk) => {
    appOutput += chunk.toString();
  });
  child.stderr.on('data', (chunk) => {
    appOutput += chunk.toString();
  });
  let client;
  let testProcessIdentities = [];
  let failure;
  let teardownFailure;
  try {
    hostIdentity = processIdentity(child.pid);
    check(hostIdentity, 'Cannot identify the launched paqet process');
    testProcessIdentities.push(hostIdentity);
    const port = await findDevToolsPort(userData, child);
    const page = await findPage(port, child);
    client = new DevToolsClient(page.webSocketDebuggerUrl);
    await waitForApplication(client);
    const version = await client.send('Browser.getVersion');
    testProcessIdentities.push(...processTreeIdentities(child.pid));

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
    await assertReachable(
      client,
      '.connect-button',
      'Primary connection action',
    );
    await assertReachable(client, '[aria-label="Log actions"]', 'Log actions');
    await assertReachable(
      client,
      '[aria-label="Connection logs"]',
      'Log surface',
    );

    await verifyClipboardApi(client);
    testProcessIdentities.push(...processTreeIdentities(child.pid));
    sendNativeInput(child, 'zoom-reset');
    await delay(300);
    const restored = await readMetrics(client);
    check(
      Math.abs(restored.devicePixelRatio - baseline.devicePixelRatio) <= 0.01,
      'Browser zoom did not return to 100%',
    );

    console.log(
      `Host: ${os.version()} ${os.arch()}, WebView2 ${version.product}`,
    );
    console.log(
      `Default: client ${baseline.innerWidth}x${baseline.innerHeight} CSS px / ${nativeMetrics.clientWidth}x${nativeMetrics.clientHeight} physical px, window ${nativeMetrics.windowWidth}x${nativeMetrics.windowHeight} physical px at ${nativeMetrics.dpi} DPI, DPR ${baseline.devicePixelRatio}, document ${baseline.scrollWidth}x${baseline.scrollHeight}`,
    );
    console.log(
      `Zoom: ${Math.round(zoomRatio * 100)}%, viewport ${zoomed.innerWidth}x${zoomed.innerHeight} CSS px, document ${zoomed.scrollWidth}x${zoomed.scrollHeight}, no horizontal overflow`,
    );
    console.log(`Keyboard: ${focused.join(' -> ')}`);
    console.log(
      'Clipboard: secure WebView2 write API is available (non-mutating)',
    );
  } catch (error) {
    if (appOutput.trim()) {
      failure = new Error(
        `${error.message}\nApplication output:\n${appOutput.trim()}`,
        {
          cause: error,
        },
      );
    } else {
      failure = error;
    }
  } finally {
    const teardownErrors = [];
    client?.close();
    if (child.exitCode === null) {
      try {
        testProcessIdentities.push(...processTreeIdentities(child.pid));
      } catch (error) {
        teardownErrors.push(error);
      }
    }
    if (child.exitCode === null) {
      try {
        sendNativeInput(child, 'close');
      } catch (error) {
        teardownErrors.push(error);
        try {
          terminateProcessTree(hostIdentity);
        } catch (killError) {
          teardownErrors.push(killError);
        }
      }
    }
    const exitCode = await waitForExit(child, 5_000);
    if (exitCode === undefined && child.exitCode === null) {
      try {
        terminateProcessTree(hostIdentity);
      } catch (error) {
        teardownErrors.push(error);
      }
      if ((await waitForExit(child, 2_000)) === undefined) {
        teardownErrors.push(
          new Error('paqet did not exit after forced teardown'),
        );
      }
    }
    try {
      const identities = [
        ...new Map(
          testProcessIdentities
            .filter(Boolean)
            .map((identity) => [
              `${identity.pid}:${identity.name}:${identity.created}`,
              identity,
            ]),
        ).values(),
      ];
      await waitForTestProcesses(identities, 5_000);
    } catch (error) {
      teardownErrors.push(error);
    }
    try {
      await rm(testRoot, {
        recursive: true,
        force: true,
        maxRetries: 10,
        retryDelay: 200,
      });
    } catch (error) {
      teardownErrors.push(error);
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
}

await main();
