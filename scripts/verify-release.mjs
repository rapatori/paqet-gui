import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, readFile, readdir, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';

const require = createRequire(import.meta.url);
const { path7z } = require('7zip-bin-full');
const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const targetTriple = 'x86_64-pc-windows-msvc';
const release = path.join(root, 'src-tauri', 'target', targetTriple, 'release');
const installer = path.join(
  release,
  'bundle',
  'nsis',
  'PaqetGUI_0.1.0_x64-setup.exe',
);
const generatedScript = path.join(release, 'nsis', 'x64', 'installer.nsi');

async function sha256File(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

async function run(command, arguments_, options = {}) {
  const child = spawn(command, arguments_, {
    cwd: root,
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
    ...options,
  });
  let stdout = '';
  let stderr = '';
  child.stdout?.setEncoding('utf8');
  child.stderr?.setEncoding('utf8');
  child.stdout?.on('data', (data) => (stdout += data));
  child.stderr?.on('data', (data) => (stderr += data));
  const result = await new Promise((resolve, reject) => {
    child.on('error', reject);
    child.on('exit', (code, signal) => resolve({ code, signal }));
  });
  if (result.signal || result.code !== 0) {
    throw new Error(
      `${path.basename(command)} failed (${result.signal ?? result.code})\n${stderr || stdout}`,
    );
  }
  return { stdout, stderr };
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

export function verifyWebviewConfiguration(config) {
  const windows = config.bundle?.windows;
  assert(
    windows?.webviewInstallMode?.type === 'skip',
    'Release installer must skip WebView2 deployment',
  );
  assert(
    !Object.hasOwn(windows, 'minimumWebview2Version'),
    'Release installer must not configure a minimum WebView2 update path',
  );
}

export function verifyProductIdentity(config) {
  assert(
    config.productName === 'PaqetGUI',
    'Release product name must be PaqetGUI',
  );
  assert(
    config.app?.windows?.length === 1 &&
      config.app.windows[0]?.label === 'main' &&
      config.app.windows[0]?.title === 'PaqetGUI',
    'Main window title must be PaqetGUI',
  );
  assert(
    config.bundle?.shortDescription ===
      'PaqetGUI is a lightweight Windows desktop client for paqet',
    'Release description must identify PaqetGUI',
  );
  assert(
    config.identifier === 'io.github.rapatori.paqet-gui',
    'Technical bundle identifier must remain stable',
  );
}

function nsisDefinition(script, name) {
  return script.match(new RegExp(`^!define ${name} "([^"]*)"$`, 'm'))?.[1];
}

export function verifyGeneratedProductIdentity(script) {
  assert(
    nsisDefinition(script, 'PRODUCTNAME') === 'PaqetGUI',
    'Generated NSIS product name must be PaqetGUI',
  );
  assert(
    nsisDefinition(script, 'MAINBINARYNAME') === 'paqet-gui',
    'Generated NSIS main binary name must remain paqet-gui',
  );
  assert(
    nsisDefinition(script, 'BUNDLEID') === 'io.github.rapatori.paqet-gui',
    'Generated NSIS bundle identifier must remain stable',
  );
  for (const declaration of [
    'VIAddVersionKey "ProductName" "${PRODUCTNAME}"',
    'VIAddVersionKey "FileDescription" "${PRODUCTNAME}"',
    'WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"',
    'CreateShortcut "$SMPROGRAMS\\${PRODUCTNAME}.lnk" "$INSTDIR\\${MAINBINARYNAME}.exe"',
  ]) {
    assert(
      script.includes(declaration),
      `Generated NSIS identity declaration is missing: ${declaration}`,
    );
  }
}

export function verifyGeneratedWebviewConfiguration(script) {
  assert(
    nsisDefinition(script, 'INSTALLWEBVIEW2MODE') === '',
    'Generated NSIS script must render skip mode as no WebView2 deployment mode',
  );
  assert(
    nsisDefinition(script, 'MINIMUMWEBVIEW2VERSION') === '',
    'Generated NSIS script must not configure a minimum WebView2 update',
  );
  for (const name of [
    'WEBVIEW2BOOTSTRAPPERPATH',
    'WEBVIEW2INSTALLERPATH',
    'WEBVIEW2FIXEDRUNTIMEPATH',
  ]) {
    const value = nsisDefinition(script, name);
    assert(
      value === undefined || value === '',
      `Generated NSIS script has an active WebView2 deployment input: ${name}`,
    );
  }
}

export function verifyNoWebviewPayload(relativePaths) {
  const webviewPayload = relativePaths.find((relative) =>
    /(?:webview2|msedgewebview|embeddedbrowserwebview)/i.test(relative),
  );
  assert(
    webviewPayload === undefined,
    `Release payload contains WebView2 deployment content: ${webviewPayload}`,
  );
}

async function assertSameFile(expected, actual, label) {
  assert(
    (await sha256File(actual)) === (await sha256File(expected)),
    `Extracted ${label} does not match its release input`,
  );
}

async function readWindowsVersionInfo(file) {
  const command = `$info = (Get-Item -LiteralPath '${file.replaceAll("'", "''")}').VersionInfo; @{ FileDescription = $info.FileDescription; ProductName = $info.ProductName } | ConvertTo-Json -Compress`;
  return JSON.parse(
    (
      await run('powershell.exe', ['-NoProfile', '-Command', command])
    ).stdout.trim(),
  );
}

async function assertWindowsProductIdentity(file, label) {
  const info = await readWindowsVersionInfo(file);
  assert(
    info.ProductName === 'PaqetGUI',
    `${label} Windows ProductName must be PaqetGUI`,
  );
  assert(
    info.FileDescription === 'PaqetGUI',
    `${label} Windows FileDescription must be PaqetGUI`,
  );
}

async function assertPackagedApplication(expected, actual) {
  const built = await readFile(expected);
  const packaged = await readFile(actual);
  const marker = Buffer.from('_TAURI_BUNDLE_TYPE_VAR_');
  const bundleTypeOffsets = [];
  for (
    let markerOffset = built.indexOf(marker);
    markerOffset >= 0;
    markerOffset = built.indexOf(marker, markerOffset + marker.length)
  ) {
    bundleTypeOffsets.push(markerOffset + marker.length);
  }
  assert(
    bundleTypeOffsets.length > 0,
    'Built application has no Tauri bundle marker',
  );
  const unbundledOffsets = bundleTypeOffsets.filter((offset) =>
    built.subarray(offset, offset + 3).equals(Buffer.from('UNK')),
  );
  assert(
    unbundledOffsets.length === 1,
    'Built application must have exactly one unbundled Tauri marker',
  );
  for (const offset of bundleTypeOffsets) {
    assert(
      packaged.subarray(offset, offset + 3).equals(Buffer.from('NSS')),
      'Packaged application does not have the NSIS Tauri marker',
    );
  }
  packaged.set(Buffer.from('UNK'), unbundledOffsets[0]);
  assert(
    packaged.equals(built),
    'Packaged application differs outside the Tauri NSIS bundle marker',
  );
}

async function verifyExtractor() {
  const formats = await run(path7z, ['i']);
  assert(
    /^\s*\d+\s+\S+\s+Nsis\s+nsis\s/m.test(formats.stdout),
    'Release extractor must support NSIS archives',
  );
}

async function verifyConfiguration() {
  const config = JSON.parse(
    await readFile(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'),
  );
  const overlay = JSON.parse(
    await readFile(
      path.join(root, 'src-tauri', 'tauri.sidecar.conf.json'),
      'utf8',
    ),
  );
  assert(
    JSON.stringify(config.bundle?.targets) === JSON.stringify(['nsis']),
    'Release bundle target must be NSIS only',
  );
  verifyProductIdentity(config);
  verifyWebviewConfiguration(config);
  assert(
    config.bundle?.windows?.nsis?.installMode === 'currentUser',
    'NSIS installer must use current-user mode',
  );
  assert(
    JSON.stringify(overlay.bundle?.externalBin) ===
      JSON.stringify(['binaries/paqet_windows_amd64']),
    'Release overlay must declare the pinned paqet sidecar',
  );
}

async function verifyNotices() {
  const required = [
    'LICENSE',
    'THIRD_PARTY_NOTICES.md',
    'licenses/FRONTEND_THIRD_PARTY_LICENSES.txt',
    'licenses/PAQET_THIRD_PARTY_LICENSES.txt',
    'licenses/RUST_THIRD_PARTY_LICENSES.txt',
    'src/assets/fonts/JETBRAINS_MONO_OFL.txt',
    'src/assets/fonts/SPACE_GROTESK_OFL.txt',
  ];
  for (const relative of required) {
    const metadata = await stat(path.join(root, relative));
    assert(
      metadata.isFile() && metadata.size > 100,
      `${relative} is incomplete`,
    );
  }

  const paqetNotices = await readFile(
    path.join(root, 'licenses', 'PAQET_THIRD_PARTY_LICENSES.txt'),
    'utf8',
  );
  const { stdout } = await run('go', [
    'version',
    '-m',
    path.join(
      root,
      'src-tauri',
      'binaries',
      `paqet_windows_amd64-${targetTriple}.exe`,
    ),
  ]);
  for (const line of stdout.split(/\r?\n/)) {
    const match = line.trim().match(/^dep\s+(\S+)\s+(\S+)/);
    if (match && match[2].startsWith('v')) {
      assert(
        paqetNotices.includes(`${match[1]} ${match[2]}`),
        `Missing paqet notice for ${match[1]} ${match[2]}`,
      );
    }
  }

  const rustNotices = await readFile(
    path.join(root, 'licenses', 'RUST_THIRD_PARTY_LICENSES.txt'),
    'utf8',
  );
  const metadata = JSON.parse(
    (
      await run('cargo', [
        'metadata',
        '--manifest-path',
        'src-tauri/Cargo.toml',
        '--locked',
        '--filter-platform',
        targetTriple,
        '--format-version',
        '1',
      ])
    ).stdout,
  );
  const rootPackage = metadata.packages.find(
    (entry) => entry.name === 'paqet-gui' && entry.source === null,
  );
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const packages = new Map(metadata.packages.map((entry) => [entry.id, entry]));
  const visited = new Set([rootPackage.id]);
  const pending = [rootPackage.id];
  while (pending.length > 0) {
    const node = nodes.get(pending.pop());
    for (const dependency of node?.deps ?? []) {
      if (!dependency.dep_kinds.some((kind) => kind.kind === null)) continue;
      if (!visited.has(dependency.pkg)) {
        visited.add(dependency.pkg);
        pending.push(dependency.pkg);
      }
    }
  }
  visited.delete(rootPackage.id);
  const noticedRustPackages = [
    ...rustNotices.matchAll(/^- (\S+) (\S+)(?: \(|$)/gm),
  ]
    .map((match) => `${match[1]}@${match[2]}`)
    .sort();
  const lockedWindowsPackages = new Set(
    [...visited]
      .map((id) => packages.get(id))
      .map((entry) => `${entry.name}@${entry.version}`),
  );
  assert(
    noticedRustPackages.every((entry) => lockedWindowsPackages.has(entry)),
    `Rust notices contain packages outside the locked Windows graph: ${noticedRustPackages.filter((entry) => !lockedWindowsPackages.has(entry)).join(', ')}`,
  );
  for (const required of [
    'serde@1.0.229',
    'serde_json@1.0.151',
    'serde_yaml_ng@0.10.0',
    'sha2@0.10.9',
    'tauri@2.11.5',
    'uuid@1.24.0',
    'windows-sys@0.61.2',
  ]) {
    assert(
      noticedRustPackages.includes(required),
      `Missing Rust notice for ${required}`,
    );
  }
}

async function verifyArtifact() {
  const extraction = path.join(
    tmpdir(),
    `paqet-gui-release-${process.pid}-${Date.now()}`,
  );
  await mkdir(extraction, { recursive: true });
  try {
    await run(path7z, ['x', '-y', `-o${extraction}`, installer]);
    const script = await readFile(generatedScript, 'utf8');
    verifyGeneratedProductIdentity(script);
    verifyGeneratedWebviewConfiguration(script);
    const declarations = [
      '!define ARCH "x64"',
      '!define INSTALLMODE "currentUser"',
      '!define ALLOWDOWNGRADES "false"',
      'File /a "/oname=paqet_windows_amd64.exe"',
      'File /a "/oname=licenses\\PAQET_GUI_LICENSE.txt"',
      'File /a "/oname=licenses\\PAQET_THIRD_PARTY_LICENSES.txt"',
      'File /a "/oname=licenses\\RUST_THIRD_PARTY_LICENSES.txt"',
      'File /a "/oname=licenses\\FRONTEND_THIRD_PARTY_LICENSES.txt"',
    ];
    for (const declaration of declarations) {
      assert(
        script.includes(declaration),
        `Missing NSIS declaration: ${declaration}`,
      );
    }

    const payload = [
      [
        path.join(
          root,
          'src-tauri',
          'binaries',
          `paqet_windows_amd64-${targetTriple}.exe`,
        ),
        'paqet_windows_amd64.exe',
        'paqet sidecar',
      ],
      [path.join(root, 'LICENSE'), 'licenses/PAQET_GUI_LICENSE.txt', 'license'],
      [
        path.join(root, 'THIRD_PARTY_NOTICES.md'),
        'licenses/THIRD_PARTY_NOTICES.md',
        'third-party notices',
      ],
      ...[
        'FRONTEND_THIRD_PARTY_LICENSES.txt',
        'PAQET_THIRD_PARTY_LICENSES.txt',
        'RUST_THIRD_PARTY_LICENSES.txt',
      ].map((name) => [
        path.join(root, 'licenses', name),
        path.join('licenses', name),
        name,
      ]),
      ...['JETBRAINS_MONO_OFL.txt', 'SPACE_GROTESK_OFL.txt'].map((name) => [
        path.join(root, 'src', 'assets', 'fonts', name),
        path.join('licenses', 'fonts', name),
        name,
      ]),
    ];
    for (const [expected, extracted, label] of payload) {
      await assertSameFile(expected, path.join(extraction, extracted), label);
    }
    await assertPackagedApplication(
      path.join(release, 'paqet-gui.exe'),
      path.join(extraction, 'paqet-gui.exe'),
    );
    await assertWindowsProductIdentity(installer, 'Installer');
    await assertWindowsProductIdentity(
      path.join(extraction, 'paqet-gui.exe'),
      'Packaged application',
    );
    const extractedPaths = (await readdir(extraction, { recursive: true }))
      .map((entry) => entry.toString())
      .sort();
    verifyNoWebviewPayload(extractedPaths);

    const expectedSidecarHash = await sha256File(
      path.join(
        root,
        'src-tauri',
        'binaries',
        `paqet_windows_amd64-${targetTriple}.exe`,
      ),
    );
    const installerMetadata = await stat(installer);
    const signature = await run('powershell.exe', [
      '-NoProfile',
      '-Command',
      `(Get-AuthenticodeSignature -LiteralPath '${installer.replaceAll("'", "''")}').Status`,
    ]);
    assert(
      signature.stdout.trim() === 'NotSigned',
      'Installer must remain unsigned',
    );
    console.log(`Installer: ${path.relative(root, installer)}`);
    console.log(`Size: ${installerMetadata.size} bytes`);
    console.log(`SHA-256: ${await sha256File(installer)}`);
    console.log('Authenticode: NotSigned');
    console.log(`Sidecar SHA-256: ${expectedSidecarHash}`);
    console.log('WebView2 deployment: skipped; no payload');
    console.log('Release payload: source-identical');
  } finally {
    await rm(extraction, { recursive: true, force: true });
  }
}

export async function verifyRelease(options = {}) {
  await verifyConfiguration();
  await verifyNotices();
  await verifyExtractor();
  if (!options.configurationOnly) await verifyArtifact();
}

const isDirectRun =
  process.argv[1] !== undefined &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;
if (isDirectRun) await verifyRelease();
