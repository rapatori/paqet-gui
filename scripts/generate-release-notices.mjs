import { execFile } from 'node:child_process';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const sidecar = path.join(
  root,
  'src-tauri',
  'binaries',
  'paqet_windows_amd64-x86_64-pc-windows-msvc.exe',
);

const paqetModules = [
  [
    'github.com/goccy/go-yaml',
    'v1.19.2',
    'https://raw.githubusercontent.com/goccy/go-yaml/v1.19.2/LICENSE',
  ],
  [
    'github.com/gopacket/gopacket',
    'v1.6.1',
    'https://raw.githubusercontent.com/gopacket/gopacket/v1.6.1/LICENSE',
  ],
  [
    'github.com/inconshreveable/mousetrap',
    'v1.1.0',
    'https://raw.githubusercontent.com/inconshreveable/mousetrap/v1.1.0/LICENSE',
  ],
  [
    'github.com/klauspost/cpuid/v2',
    'v2.3.0',
    'https://raw.githubusercontent.com/klauspost/cpuid/v2.3.0/LICENSE',
  ],
  [
    'github.com/klauspost/reedsolomon',
    'v1.13.0',
    'https://raw.githubusercontent.com/klauspost/reedsolomon/v1.13.0/LICENSE',
  ],
  [
    'github.com/pkg/errors',
    'v0.9.1',
    'https://raw.githubusercontent.com/pkg/errors/v0.9.1/LICENSE',
  ],
  [
    'github.com/spf13/cobra',
    'v1.10.2',
    'https://raw.githubusercontent.com/spf13/cobra/v1.10.2/LICENSE.txt',
  ],
  [
    'github.com/spf13/pflag',
    'v1.0.10',
    'https://raw.githubusercontent.com/spf13/pflag/v1.0.10/LICENSE',
  ],
  [
    'github.com/tjfoc/gmsm',
    'v1.4.1',
    'https://raw.githubusercontent.com/tjfoc/gmsm/v1.4.1/LICENSE',
  ],
  [
    'github.com/xtaci/kcp-go/v5',
    'v5.6.64',
    'https://raw.githubusercontent.com/xtaci/kcp-go/v5.6.64/LICENSE',
  ],
  [
    'github.com/xtaci/smux',
    'v1.5.53',
    'https://raw.githubusercontent.com/xtaci/smux/v1.5.53/LICENSE',
  ],
  [
    'golang.org/x/crypto',
    'v0.53.0',
    'https://raw.githubusercontent.com/golang/crypto/v0.53.0/LICENSE',
  ],
  [
    'golang.org/x/net',
    'v0.55.0',
    'https://raw.githubusercontent.com/golang/net/v0.55.0/LICENSE',
  ],
  [
    'golang.org/x/sys',
    'v0.46.0',
    'https://raw.githubusercontent.com/golang/sys/v0.46.0/LICENSE',
  ],
  [
    'golang.org/x/time',
    'v0.14.0',
    'https://raw.githubusercontent.com/golang/time/v0.14.0/LICENSE',
  ],
];

const additionalAttribution = new Map([
  ['github.com/spf13/cobra', 'Copyright 2013-2023 The Cobra Authors'],
  [
    'github.com/tjfoc/gmsm',
    'Copyright 2017- Suzhou Tongji Fintech Research Institute. All Rights Reserved.\nCopyright Hyperledger-TWGC All Rights Reserved.',
  ],
]);

function normalize(text) {
  return text.replaceAll('\r\n', '\n').trimEnd();
}

async function writeAtomic(file, content) {
  const temporary = `${file}.${process.pid}.tmp`;
  await writeFile(temporary, `${normalize(content)}\n`, 'utf8');
  await rename(temporary, file);
}

async function sidecarModules() {
  const { stdout } = await execFileAsync('go', ['version', '-m', sidecar], {
    cwd: root,
    windowsHide: true,
  });
  return stdout
    .split(/\r?\n/)
    .map((line) => line.trim().split(/\s+/))
    .filter(([kind, , version]) => kind === 'dep' && version?.startsWith('v'))
    .map(([, module, version]) => `${module}@${version}`)
    .sort();
}

async function fetchLicense(url) {
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok) {
    throw new Error(`Cannot fetch ${url}: HTTP ${response.status}`);
  }
  const text = normalize(await response.text());
  if (text.length < 100 || /<html/i.test(text)) {
    throw new Error(`Unexpected license response from ${url}`);
  }
  return text;
}

async function generatePaqetNotices() {
  const expected = paqetModules
    .map(([module, version]) => `${module}@${version}`)
    .sort();
  const actual = await sidecarModules();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      'The shipped paqet module inventory differs from the reviewed notice inventory',
    );
  }

  const sections = await Promise.all(
    paqetModules.map(async ([module, version, licenseUrl]) => {
      const attribution = additionalAttribution.get(module);
      return [
        '-'.repeat(79),
        `${module} ${version}`,
        `Source: https://${module}`,
        `License source: ${licenseUrl}`,
        attribution ? `\n${attribution}` : '',
        '',
        await fetchLicense(licenseUrl),
      ]
        .filter((line) => line !== '')
        .join('\n');
    }),
  );

  const output = [
    'Third-Party Licenses Included in the Pinned paqet Executable',
    '='.repeat(61),
    'This inventory matches the module metadata embedded in the shipped',
    '`paqet_windows_amd64.exe` for paqet v1.0.0-alpha.20. The Go standard',
    'library is distributed under the Go project license and is not listed as',
    'a module by `go version -m`; its license is included after the modules.',
    '',
    ...sections,
    '-'.repeat(79),
    'Go standard library (Go 1.26.4)',
    'Source: https://go.dev/',
    'License source: https://raw.githubusercontent.com/golang/go/go1.26.4/LICENSE',
    '',
    await fetchLicense(
      'https://raw.githubusercontent.com/golang/go/go1.26.4/LICENSE',
    ),
  ].join('\n');

  await writeAtomic(
    path.join(root, 'licenses', 'PAQET_THIRD_PARTY_LICENSES.txt'),
    output,
  );
}

async function generateFrontendNotices() {
  const packages = [
    {
      name: '@tauri-apps/api',
      version: '2.11.1',
      source: 'https://github.com/tauri-apps/tauri',
      license: path.join('node_modules', '@tauri-apps', 'api', 'LICENSE_MIT'),
    },
    {
      name: 'svelte',
      version: '5.56.7',
      source: 'https://github.com/sveltejs/svelte',
      license: path.join('node_modules', 'svelte', 'LICENSE.md'),
    },
  ];

  const sections = await Promise.all(
    packages.map(async (entry) => {
      const metadata = JSON.parse(
        await readFile(
          path.join(root, 'node_modules', entry.name, 'package.json'),
          'utf8',
        ),
      );
      if (metadata.version !== entry.version) {
        throw new Error(
          `${entry.name} version ${metadata.version} differs from reviewed ${entry.version}`,
        );
      }
      return [
        '-'.repeat(79),
        `${entry.name} ${entry.version}`,
        `Source: ${entry.source}`,
        '',
        normalize(await readFile(path.join(root, entry.license), 'utf8')),
      ].join('\n');
    }),
  );

  await writeAtomic(
    path.join(root, 'licenses', 'FRONTEND_THIRD_PARTY_LICENSES.txt'),
    [
      'Third-Party Frontend Runtime Licenses',
      '='.repeat(37),
      'This inventory covers packages whose code is included in the production',
      'frontend bundle. Build, lint, and test-only packages are excluded.',
      '',
      ...sections,
    ].join('\n'),
  );
}

await mkdir(path.join(root, 'licenses'), { recursive: true });
await Promise.all([generatePaqetNotices(), generateFrontendNotices()]);
