import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { buildSidecar } from './build-sidecar.mjs';

const bytes = Buffer.from('pinned sidecar fixture');
const artifact = {
  executableName: 'paqet_windows_amd64.exe',
  executableSize: bytes.length,
  executableSha256: createHash('sha256').update(bytes).digest('hex'),
  targetTriple: 'x86_64-pc-windows-msvc',
  tauriSidecarStem: 'binaries/paqet_windows_amd64',
};
const contract = { windowsAmd64Artifact: artifact };
const sidecarConfig = {
  bundle: { externalBin: [artifact.tauriSidecarStem] },
};

async function fixture(t) {
  const root = path.join(
    tmpdir(),
    `paqet-gui-sidecar-${process.pid}-${Date.now()}-${Math.random()}`,
  );
  const binaries = path.join(root, 'src-tauri', 'binaries');
  await mkdir(binaries, { recursive: true });
  t.after(() => rm(root, { recursive: true, force: true }));
  return {
    root,
    input: path.join(
      binaries,
      'paqet_windows_amd64-x86_64-pc-windows-msvc.exe',
    ),
    output: path.join(
      root,
      'src-tauri',
      'target',
      'debug',
      artifact.executableName,
    ),
  };
}

function options(root, overrides = {}) {
  return {
    root,
    arguments: ['--debug', '--no-bundle'],
    platform: 'win32',
    architecture: 'x64',
    contract,
    sidecarConfig,
    runTauri: async () => ({ code: 0, signal: null }),
    ...overrides,
  };
}

test('rejects absent, directory, wrong-size, and wrong-hash inputs', async (t) => {
  const { root, input } = await fixture(t);
  await assert.rejects(buildSidecar(options(root)), /is not staged/);

  await mkdir(input);
  await assert.rejects(buildSidecar(options(root)), /unexpected type or size/);
  await rm(input, { recursive: true });

  await writeFile(input, Buffer.from('short'));
  await assert.rejects(buildSidecar(options(root)), /unexpected type or size/);

  await writeFile(input, Buffer.alloc(bytes.length));
  await assert.rejects(buildSidecar(options(root)), /SHA-256 mismatch/);
});

test('rejects symbolic-link inputs', async (t) => {
  const { root, input } = await fixture(t);
  if (process.platform === 'win32') {
    const directory = path.join(root, 'sidecar-target');
    const target = path.join(directory, path.basename(input));
    await mkdir(directory);
    await writeFile(target, bytes);
    await symlink(directory, input, 'junction');
  } else {
    const target = path.join(root, 'sidecar-target.exe');
    await writeFile(target, bytes);
    await symlink(target, input, 'file');
  }
  await assert.rejects(buildSidecar(options(root)), /unexpected type or size/);
});

test('rejects platform, target, and overlay contract drift before launch', async (t) => {
  const { root, input } = await fixture(t);
  await writeFile(input, bytes);

  await assert.rejects(
    buildSidecar(options(root, { platform: 'linux' })),
    /requires Windows x64/,
  );
  await assert.rejects(
    buildSidecar(
      options(root, { arguments: ['--target', 'aarch64-pc-windows-msvc'] }),
    ),
    /target must be x86_64-pc-windows-msvc/,
  );
  await assert.rejects(
    buildSidecar(
      options(root, {
        sidecarConfig: { bundle: { externalBin: ['binaries/other'] } },
      }),
    ),
    /externalBin does not match/,
  );
  await assert.rejects(
    buildSidecar(
      options(root, {
        contract: {
          windowsAmd64Artifact: {
            ...artifact,
            executableName: 'other.exe',
          },
        },
      }),
    ),
    /executable name does not match/,
  );
});

test('preserves child failure without inspecting a copied artifact', async (t) => {
  const { root, input } = await fixture(t);
  await writeFile(input, bytes);

  const result = await buildSidecar(
    options(root, { runTauri: async () => ({ code: 17, signal: null }) }),
  );
  assert.deepEqual(result, { code: 17, signal: null });
});

test('verifies the copied sidecar after a successful build', async (t) => {
  const { root, input } = await fixture(t);
  const output = path.join(
    root,
    'src-tauri',
    'target',
    artifact.targetTriple,
    'debug',
    artifact.executableName,
  );
  await writeFile(input, bytes);

  let invocation;
  const result = await buildSidecar(
    options(root, {
      arguments: ['-d', '--target', artifact.targetTriple, '--no-bundle'],
      runTauri: async (invocationRoot, arguments_) => {
        invocation = { invocationRoot, arguments_ };
        await mkdir(path.dirname(output), { recursive: true });
        await writeFile(output, bytes);
        return { code: 0, signal: null };
      },
    }),
  );

  assert.deepEqual(invocation, {
    invocationRoot: root,
    arguments_: ['-d', '--target', artifact.targetTriple, '--no-bundle'],
  });
  assert.deepEqual(result, { code: 0, signal: null });
});

test('rejects a successful build that did not copy the pinned sidecar', async (t) => {
  const { root, input } = await fixture(t);
  await writeFile(input, bytes);

  await assert.rejects(
    buildSidecar(options(root)),
    /Built paqet sidecar identity mismatch/,
  );
});
