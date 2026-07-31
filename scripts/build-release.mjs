import { buildSidecar } from './build-sidecar.mjs';
import { verifyRelease } from './verify-release.mjs';

const arguments_ = [
  '--target',
  'x86_64-pc-windows-msvc',
  '--bundles',
  'nsis',
  '--ci',
  '--no-sign',
];

const result = await buildSidecar({ arguments: arguments_ });
if (result.signal) {
  process.kill(process.pid, result.signal);
} else if (result.code !== 0) {
  process.exitCode = result.code ?? 1;
} else {
  await verifyRelease();
}
