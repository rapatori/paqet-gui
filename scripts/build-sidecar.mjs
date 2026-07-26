import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstat, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const defaultRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));

function requireString(value, field) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`Compatibility manifest field ${field} must be text`);
  }
  return value;
}

function requireSize(value, field) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`Compatibility manifest field ${field} must be a size`);
  }
  return value;
}

function requireSha256(value, field) {
  if (typeof value !== 'string' || !/^[a-f0-9]{64}$/.test(value)) {
    throw new Error(`Compatibility manifest field ${field} must be a SHA-256`);
  }
  return value;
}

async function readJson(file, description) {
  try {
    return JSON.parse(await readFile(file, 'utf8'));
  } catch (error) {
    throw new Error(`Cannot read ${description} at ${file}`, { cause: error });
  }
}

export async function verifyExecutable(file, expectedSize, expectedSha256) {
  let metadata;
  try {
    metadata = await lstat(file);
  } catch (error) {
    throw new Error(`Pinned paqet sidecar is not staged at ${file}`, {
      cause: error,
    });
  }
  if (!metadata.isFile() || metadata.size !== expectedSize) {
    throw new Error(
      `Pinned paqet sidecar has unexpected type or size at ${file}`,
    );
  }

  const digest = createHash('sha256')
    .update(await readFile(file))
    .digest('hex');
  if (digest !== expectedSha256) {
    throw new Error(`Pinned paqet sidecar SHA-256 mismatch at ${file}`);
  }
}

function argumentValue(arguments_, longName, shortName) {
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === longName || argument === shortName) {
      return arguments_[index + 1];
    }
    if (argument.startsWith(`${longName}=`)) {
      return argument.slice(longName.length + 1);
    }
  }
  return undefined;
}

function spawnTauri(root, arguments_) {
  const tauriCli = path.join(
    root,
    'node_modules',
    '@tauri-apps',
    'cli',
    'tauri.js',
  );
  const child = spawn(
    process.execPath,
    [
      tauriCli,
      'build',
      '--config',
      'src-tauri/tauri.sidecar.conf.json',
      ...arguments_,
    ],
    { cwd: root, stdio: 'inherit' },
  );
  return new Promise((resolve, reject) => {
    child.on('error', reject);
    child.on('exit', (code, signal) => resolve({ code, signal }));
  });
}

export async function buildSidecar(options = {}) {
  const root = options.root ?? defaultRoot;
  const arguments_ = options.arguments ?? process.argv.slice(2);
  const platform = options.platform ?? process.platform;
  const architecture = options.architecture ?? process.arch;
  const contract =
    options.contract ??
    (await readJson(
      path.join(root, 'src-tauri', 'compat', 'paqet-v1.0.0-alpha.20.json'),
      'paqet compatibility manifest',
    ));
  const sidecarConfig =
    options.sidecarConfig ??
    (await readJson(
      path.join(root, 'src-tauri', 'tauri.sidecar.conf.json'),
      'Tauri sidecar configuration',
    ));
  const artifact = contract.windowsAmd64Artifact;
  if (!artifact || typeof artifact !== 'object') {
    throw new Error(
      'Compatibility manifest field windowsAmd64Artifact must be an object',
    );
  }

  const executableName = requireString(
    artifact.executableName,
    'windowsAmd64Artifact.executableName',
  );
  const expectedSize = requireSize(
    artifact.executableSize,
    'windowsAmd64Artifact.executableSize',
  );
  const expectedSha256 = requireSha256(
    artifact.executableSha256,
    'windowsAmd64Artifact.executableSha256',
  );
  const targetTriple = requireString(
    artifact.targetTriple,
    'windowsAmd64Artifact.targetTriple',
  );
  const sidecarStem = requireString(
    artifact.tauriSidecarStem,
    'windowsAmd64Artifact.tauriSidecarStem',
  );

  if (platform !== 'win32' || architecture !== 'x64') {
    throw new Error('The pinned paqet sidecar build requires Windows x64');
  }
  if (targetTriple !== 'x86_64-pc-windows-msvc') {
    throw new Error(`Unsupported paqet sidecar target ${targetTriple}`);
  }
  if (
    JSON.stringify(sidecarConfig.bundle?.externalBin) !==
    JSON.stringify([sidecarStem])
  ) {
    throw new Error(
      'Tauri externalBin does not match the compatibility manifest',
    );
  }
  if (executableName !== `${path.basename(sidecarStem)}.exe`) {
    throw new Error(
      'Paqet executable name does not match the Tauri sidecar stem',
    );
  }

  const requestedTarget = argumentValue(arguments_, '--target', '-t');
  if (requestedTarget !== undefined && requestedTarget !== targetTriple) {
    throw new Error(`Sidecar build target must be ${targetTriple}`);
  }
  const inputName = `${path.basename(sidecarStem)}-${targetTriple}.exe`;
  const executable = path.join(root, 'src-tauri', 'binaries', inputName);
  await verifyExecutable(executable, expectedSize, expectedSha256);

  const runTauri = options.runTauri ?? spawnTauri;
  const childResult = await runTauri(root, arguments_);
  if (childResult.signal || childResult.code !== 0) {
    return childResult;
  }

  const profile = arguments_.some(
    (argument) => argument === '--debug' || argument === '-d',
  )
    ? 'debug'
    : 'release';
  const targetSegments = requestedTarget ? [requestedTarget] : [];
  const copiedSidecar = path.join(
    root,
    'src-tauri',
    'target',
    ...targetSegments,
    profile,
    executableName,
  );
  try {
    await verifyExecutable(copiedSidecar, expectedSize, expectedSha256);
  } catch (error) {
    throw new Error(
      `Built paqet sidecar identity mismatch at ${copiedSidecar}`,
      {
        cause: error,
      },
    );
  }
  return childResult;
}

const isDirectRun =
  process.argv[1] !== undefined &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;
if (isDirectRun) {
  const childResult = await buildSidecar();
  if (childResult.signal) {
    process.kill(process.pid, childResult.signal);
  } else if (childResult.code !== 0) {
    process.exitCode = childResult.code ?? 1;
  }
}
