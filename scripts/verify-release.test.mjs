import test from 'node:test';

import assert from 'node:assert/strict';

import {
  verifyGeneratedProductIdentity,
  verifyGeneratedWebviewConfiguration,
  verifyNoWebviewPayload,
  verifyProductIdentity,
  verifyRelease,
  verifyWebviewConfiguration,
} from './verify-release.mjs';

test('release configuration and notices match locked inputs', async () => {
  await verifyRelease({ configurationOnly: true });
});

test('release identity presents PaqetGUI without changing the bundle identifier', () => {
  const config = {
    productName: 'PaqetGUI',
    identifier: 'io.github.rapatori.paqet-gui',
    app: { windows: [{ label: 'main', title: 'PaqetGUI' }] },
    bundle: {
      shortDescription:
        'PaqetGUI is a lightweight Windows desktop client for paqet',
    },
  };
  verifyProductIdentity(config);

  for (const candidate of [
    { ...config, productName: 'paqet' },
    { ...config, identifier: 'io.github.rapatori.PaqetGUI' },
    { ...config, app: { windows: [{ label: 'main', title: 'paqet' }] } },
    {
      ...config,
      bundle: { shortDescription: 'A Windows desktop client for paqet' },
    },
  ]) {
    assert.throws(() => verifyProductIdentity(candidate));
  }
});

test('generated NSIS identity presents PaqetGUI with stable technical names', () => {
  const identityDeclarations = [
    '!define PRODUCTNAME "PaqetGUI"',
    '!define MAINBINARYNAME "paqet-gui"',
    '!define BUNDLEID "io.github.rapatori.paqet-gui"',
    'VIAddVersionKey "ProductName" "${PRODUCTNAME}"',
    'VIAddVersionKey "FileDescription" "${PRODUCTNAME}"',
    'WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"',
    'CreateShortcut "$SMPROGRAMS\\${PRODUCTNAME}.lnk" "$INSTDIR\\${MAINBINARYNAME}.exe"',
  ].join('\n');
  verifyGeneratedProductIdentity(identityDeclarations);

  for (const candidate of [
    identityDeclarations.replace('"PaqetGUI"', '"paqet"'),
    identityDeclarations.replace('"paqet-gui"', '"PaqetGUI"'),
    identityDeclarations.replace(
      '"io.github.rapatori.paqet-gui"',
      '"io.github.rapatori.PaqetGUI"',
    ),
    identityDeclarations.replace(
      'VIAddVersionKey "ProductName" "${PRODUCTNAME}"',
      '',
    ),
  ]) {
    assert.throws(() => verifyGeneratedProductIdentity(candidate));
  }
});

test('release configuration rejects WebView2 deployment and update modes', () => {
  for (const webviewInstallMode of [
    { type: 'downloadBootstrapper' },
    { type: 'embedBootstrapper' },
    { type: 'offlineInstaller' },
    { type: 'fixedRuntime', path: 'runtime' },
  ]) {
    assert.throws(
      () =>
        verifyWebviewConfiguration({
          bundle: { windows: { webviewInstallMode } },
        }),
      /skip WebView2 deployment/,
    );
  }
  assert.throws(
    () =>
      verifyWebviewConfiguration({
        bundle: {
          windows: {
            webviewInstallMode: { type: 'skip' },
            minimumWebview2Version: '120.0.0.0',
          },
        },
      }),
    /minimum WebView2 update path/,
  );
});

test('generated NSIS configuration has no active WebView2 deployment input', () => {
  const skipDeclarations = `
!define INSTALLWEBVIEW2MODE ""
!define WEBVIEW2BOOTSTRAPPERPATH ""
!define WEBVIEW2INSTALLERPATH ""
!define MINIMUMWEBVIEW2VERSION ""
`;
  verifyGeneratedWebviewConfiguration(skipDeclarations);

  for (const declaration of [
    '!define INSTALLWEBVIEW2MODE "offlineInstaller"',
    '!define WEBVIEW2BOOTSTRAPPERPATH "C:\\WebView2Setup.exe"',
    '!define WEBVIEW2INSTALLERPATH "C:\\WebView2RuntimeInstaller.exe"',
    '!define WEBVIEW2FIXEDRUNTIMEPATH "C:\\WebView2Runtime"',
    '!define MINIMUMWEBVIEW2VERSION "120.0.0.0"',
  ]) {
    const name = declaration.match(/^!define (\S+)/)[1];
    const pattern = new RegExp(`^!define ${name} .*?$`, 'm');
    const candidate = pattern.test(skipDeclarations)
      ? skipDeclarations.replace(pattern, declaration)
      : `${skipDeclarations}${declaration}\n`;
    assert.throws(
      () => verifyGeneratedWebviewConfiguration(candidate),
      /WebView2/,
    );
  }
});

test('extracted payload rejects WebView2 installers and fixed runtime files', () => {
  verifyNoWebviewPayload(['paqet-gui.exe', 'licenses/THIRD_PARTY_NOTICES.md']);
  for (const relative of [
    '$TEMP/MicrosoftEdgeWebView2RuntimeInstaller.exe',
    '$TEMP/MicrosoftEdgeWebview2Setup.exe',
    'WebView2Runtime/msedgewebview2.exe',
    'WebView2Runtime/EmbeddedBrowserWebView.dll',
  ]) {
    assert.throws(() => verifyNoWebviewPayload([relative]), /WebView2/);
  }
});
