import test from 'node:test';

import { verifyRelease } from './verify-release.mjs';

test('release configuration and notices match locked inputs', async () => {
  await verifyRelease({ configurationOnly: true });
});
