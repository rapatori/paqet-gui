import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { vi } from 'vitest';
import App, { type AppApi } from './App.svelte';
import type {
  AdvancedSettings,
  AppSnapshot,
  NetworkInterface,
  Profile,
  ProfileDraft,
  RuntimeEvent,
  WindowCloseRequest,
} from './lib/api';

const styles = readFileSync(join(process.cwd(), 'src', 'styles.css'), 'utf8');
const notices = readFileSync(
  join(process.cwd(), 'THIRD_PARTY_NOTICES.md'),
  'utf8',
);
const spaceGroteskPath = join(
  process.cwd(),
  'src',
  'assets',
  'fonts',
  'space-grotesk-latin-variable.woff2',
);
const jetBrainsMonoPath = join(
  process.cwd(),
  'src',
  'assets',
  'fonts',
  'jetbrains-mono-latin-variable.woff2',
);
const spaceGroteskLicense = readFileSync(
  join(process.cwd(), 'src', 'assets', 'fonts', 'SPACE_GROTESK_OFL.txt'),
  'utf8',
);
const jetBrainsMonoLicense = readFileSync(
  join(process.cwd(), 'src', 'assets', 'fonts', 'JETBRAINS_MONO_OFL.txt'),
  'utf8',
);
const tauriConfig = JSON.parse(
  readFileSync(join(process.cwd(), 'src-tauri', 'tauri.conf.json'), 'utf8'),
) as {
  bundle: { resources: Record<string, string> };
};

function sha256(path: string) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

const primaryProfile: Profile = {
  id: '11111111-1111-4111-8111-111111111111',
  name: 'Primary',
  serverHost: '198.51.100.10',
  port: 9999,
  encryptionKey: 'representative-test-key',
};

const backupProfile: Profile = {
  id: '22222222-2222-4222-8222-222222222222',
  name: 'Backup',
  serverHost: 'backup.example.com',
  port: 443,
  encryptionKey: 'backup-test-key',
};

const ethernetInterface: NetworkInterface = {
  friendlyName: 'Ethernet',
  interfaceName: 'Ethernet',
  guid: '\\Device\\NPF_{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}',
  localAddress: '192.0.2.20',
  gatewayAddress: '192.0.2.1',
  gatewayMac: '00:11:22:33:44:55',
};

const wifiInterface: NetworkInterface = {
  friendlyName: 'Wi-Fi',
  interfaceName: 'Wi-Fi',
  guid: '\\Device\\NPF_{BBBBBBBB-CCCC-4DDD-8EEE-FFFFFFFFFFFF}',
  localAddress: '198.51.100.20',
  gatewayAddress: '198.51.100.1',
  gatewayMac: '66:77:88:99:AA:BB',
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function snapshot(
  selectedProfile: Profile | null = primaryProfile,
  overrides: Partial<AppSnapshot> = {},
): AppSnapshot {
  const profiles = [primaryProfile, backupProfile];
  return {
    revision: '12',
    profiles: selectedProfile
      ? profiles.map((profile) => ({
          id: profile.id,
          name: profile.name,
          serverHost: profile.serverHost,
          port: profile.port,
        }))
      : [],
    selectedProfile,
    interfaces: [ethernetInterface, wifiInterface],
    selectedInterfaceGuid: ethernetInterface.guid,
    advancedSettings: {
      logLevel: null,
      pcapSocketBuffer: null,
      localTcpFlags: null,
      remoteTcpFlags: null,
      connectionCount: null,
      tcpBuffer: null,
      udpBuffer: null,
      kcpMode: null,
      manualKcp: {
        noDelay: null,
        interval: null,
        resend: null,
        noCongestion: null,
        writeDelay: null,
        ackNoDelay: null,
      },
      kcpMtu: null,
      kcpReceiveWindow: null,
      kcpSendWindow: null,
      kcpBlock: null,
      smuxBuffer: null,
      streamBuffer: null,
      smuxKeepalive: null,
      smuxTimeout: null,
    },
    lifecycle: {
      status: 'disconnected',
      process: 'absent',
      failure: null,
      settingsEditable: true,
    },
    ...overrides,
  };
}

function advancedSettings(
  overrides: Partial<AdvancedSettings> = {},
): AdvancedSettings {
  return {
    ...snapshot().advancedSettings,
    ...overrides,
  };
}

function mockApi(initialSnapshot = snapshot()): AppApi {
  return {
    getAppSnapshot: vi.fn().mockResolvedValue(initialSnapshot),
    createProfile: vi.fn().mockResolvedValue(initialSnapshot),
    updateProfile: vi.fn().mockResolvedValue(initialSnapshot),
    deleteProfile: vi.fn().mockResolvedValue(initialSnapshot),
    selectProfile: vi.fn().mockResolvedValue(initialSnapshot),
    refreshInterfaces: vi.fn().mockResolvedValue(initialSnapshot),
    selectInterface: vi.fn().mockResolvedValue(initialSnapshot),
    replaceAdvancedSettings: vi.fn().mockResolvedValue(initialSnapshot),
    connect: vi.fn().mockResolvedValue(initialSnapshot),
    disconnect: vi.fn().mockResolvedValue(initialSnapshot),
    subscribeRuntimeEvents: vi.fn().mockResolvedValue(undefined),
    onWindowCloseRequested: vi.fn().mockResolvedValue(undefined),
    cancelWindowClose: vi.fn().mockResolvedValue(undefined),
    confirmWindowClose: vi.fn().mockResolvedValue(undefined),
  };
}

function runtimeCallback(api: AppApi): (event: RuntimeEvent) => void {
  return vi.mocked(api.subscribeRuntimeEvents).mock.calls[0][0];
}

function closeCallback(api: AppApi): (request: WindowCloseRequest) => void {
  return vi.mocked(api.onWindowCloseRequested).mock.calls[0][0];
}

async function renderLoaded(api = mockApi()) {
  const user = userEvent.setup();
  render(App, { props: { api } });
  await screen.findByRole('combobox', { name: 'Selected server profile' });
  return { api, user };
}

describe('application shell', () => {
  it('loads the selected profile into a calm disconnected shell with the key masked', async () => {
    const { api, user } = await renderLoaded();

    expect(api.getAppSnapshot).toHaveBeenCalledOnce();
    expect(
      screen.getByRole('heading', { level: 1, name: 'paqet' }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText('Connection status')).toHaveTextContent(
      'Disconnected',
    );
    expect(screen.getByRole('button', { name: 'Connect' })).toBeEnabled();
    expect(
      screen.getByRole('log', { name: 'Connection logs' }),
    ).toHaveTextContent('Connection output will appear here.');

    const key = screen.getByLabelText('Encryption key');
    expect(key).toHaveAttribute('type', 'password');
    expect(key).toHaveValue('representative-test-key');
    expect(key).toHaveAttribute('readonly');

    await user.click(
      screen.getByRole('button', { name: 'Reveal encryption key' }),
    );
    expect(key).toHaveAttribute('type', 'text');
    expect(
      screen.getByRole('button', { name: 'Conceal encryption key' }),
    ).toHaveAttribute('aria-pressed', 'true');

    const advanced = screen.getByRole('button', { name: /Advanced/ });
    expect(advanced).toHaveAttribute('aria-expanded', 'false');
    await user.click(advanced);
    expect(advanced).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('combobox', { name: 'Interface' })).toHaveValue(
      ethernetInterface.guid,
    );
    expect(screen.getByText('Ethernet · 192.0.2.20')).toBeInTheDocument();
    expect(screen.getByText(ethernetInterface.guid)).toBeInTheDocument();
    expect(screen.getByText('00:11:22:33:44:55')).toBeInTheDocument();
  });

  it('drives canonical connection controls through all process-aware lifecycle states', async () => {
    const api = mockApi();
    const { user } = await renderLoaded(api);
    const connect = screen.getByRole('button', { name: 'Connect' });

    await user.click(connect);
    expect(api.connect).toHaveBeenCalledOnce();

    runtimeCallback(api)({
      kind: 'lifecycle',
      revision: '9007199254740993',
      sessionId: '1',
      lifecycle: {
        status: 'connecting',
        process: 'absent',
        failure: null,
        settingsEditable: false,
      },
    });
    expect(
      await screen.findByRole('button', { name: 'Connecting…' }),
    ).toBeDisabled();
    await waitFor(() =>
      expect(screen.getByLabelText('Connection status')).toHaveTextContent(
        'Connecting',
      ),
    );

    runtimeCallback(api)({
      kind: 'lifecycle',
      revision: '9007199254740994',
      sessionId: '1',
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
    const disconnect = await screen.findByRole('button', {
      name: 'Disconnect',
    });
    expect(disconnect).toBeEnabled();
    await user.click(disconnect);
    expect(api.disconnect).toHaveBeenCalledOnce();

    runtimeCallback(api)({
      kind: 'lifecycle',
      revision: '9007199254740995',
      sessionId: '1',
      lifecycle: {
        status: 'disconnecting',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
    expect(
      await screen.findByRole('button', { name: 'Disconnecting…' }),
    ).toBeDisabled();
  });

  it('keeps a failed running process locked and exposes its canonical reason', async () => {
    const api = mockApi();
    await renderLoaded(api);

    runtimeCallback(api)({
      kind: 'output',
      revision: '13',
      sessionId: '1',
      lifecycle: {
        status: 'failed',
        process: 'running',
        failure: { kind: 'connectionLost' },
        settingsEditable: false,
      },
      record: {
        sequence: '1',
        stream: 'stdout',
        text: 'connection lost, retrying....',
        classification: { kind: 'connectionLost' },
        truncated: false,
      },
    });

    await waitFor(() =>
      expect(screen.getByLabelText('Connection status')).toHaveTextContent(
        'Failed',
      ),
    );
    expect(
      screen.getByText(
        'The paqet client reported that the connection was lost.',
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Disconnect' })).toBeEnabled();
    expect(
      screen.getByRole('combobox', { name: 'Selected server profile' }),
    ).toBeDisabled();
  });

  it('renders ordered replay gaps, stderr, and truncation and copies visible logs', async () => {
    const api = mockApi();
    const { user } = await renderLoaded(api);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });

    runtimeCallback(api)({
      kind: 'bootstrap',
      revision: '13',
      sessionId: '7',
      lifecycle: snapshot().lifecycle,
      gap: { firstMissing: '1', nextAvailable: '3' },
      records: [
        {
          sequence: '5',
          stream: 'stderr',
          text: 'later error',
          classification: { kind: 'display' },
          truncated: true,
        },
        {
          sequence: '3',
          stream: 'stdout',
          text: 'first retained',
          classification: { kind: 'display' },
          truncated: false,
        },
      ],
    });

    const log = screen.getByRole('log', { name: 'Connection logs' });
    await waitFor(() =>
      expect(log).toHaveTextContent('Output unavailable: sequences 1–2.'),
    );
    expect(log).toHaveTextContent('Output unavailable: sequences 4–4.');
    expect(within(log).getByText('stderr')).toBeInTheDocument();
    expect(within(log).getByText('record truncated')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Copy' }));
    expect(writeText).toHaveBeenCalledWith(
      '[output unavailable: sequences 1–2]\nfirst retained\n[output unavailable: sequences 4–4]\n[stderr] later error [record truncated]',
    );
    expect(screen.getByText('Logs copied.')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Clear' }));
    expect(log).toHaveTextContent('Connection output will appear here.');
    expect(api.disconnect).not.toHaveBeenCalled();
  });

  it('pauses log follow when scrolled upward and jumps to the latest output', async () => {
    const api = mockApi();
    const { user } = await renderLoaded(api);
    runtimeCallback(api)({
      kind: 'bootstrap',
      revision: '12',
      sessionId: '1',
      lifecycle: snapshot().lifecycle,
      gap: null,
      records: [
        {
          sequence: '1',
          stream: 'stdout',
          text: 'existing output',
          classification: { kind: 'display' },
          truncated: false,
        },
      ],
    });
    await screen.findByText('existing output');
    const log = screen.getByRole('log', {
      name: 'Connection logs',
    }) as HTMLDivElement;
    Object.defineProperties(log, {
      scrollHeight: { configurable: true, value: 300 },
      clientHeight: { configurable: true, value: 100 },
      scrollTop: { configurable: true, writable: true, value: 50 },
    });

    await fireEvent.scroll(log);
    runtimeCallback(api)({
      kind: 'output',
      revision: '13',
      sessionId: '1',
      lifecycle: snapshot().lifecycle,
      record: {
        sequence: '2',
        stream: 'stdout',
        text: 'new output',
        classification: { kind: 'display' },
        truncated: false,
      },
    });

    await waitFor(() => expect(log.scrollTop).toBe(50));
    const jump = await screen.findByRole('button', { name: 'Jump to latest' });
    await user.click(jump);
    expect(log.scrollTop).toBe(300);
    expect(screen.queryByRole('button', { name: 'Jump to latest' })).toBeNull();
  });

  it('confirms or cancels supervised window close with safe modal focus', async () => {
    const api = mockApi();
    const { user } = await renderLoaded(api);
    const request: WindowCloseRequest = {
      requestId: '9007199254740993',
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    };

    closeCallback(api)(request);
    const dialog = await screen.findByRole('alertdialog', {
      name: 'Disconnect and close?',
    });
    const keepOpen = within(dialog).getByRole('button', { name: 'Keep open' });
    expect(keepOpen).toHaveFocus();
    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(api.cancelWindowClose).toHaveBeenCalledWith(request.requestId),
    );
    expect(screen.queryByRole('alertdialog')).toBeNull();

    closeCallback(api)(request);
    await user.click(
      await screen.findByRole('button', { name: 'Disconnect and close' }),
    );
    expect(api.confirmWindowClose).toHaveBeenCalledWith(request.requestId);
  });

  it('does not let an older close cancellation dismiss a newer request', async () => {
    const api = mockApi();
    const cancellation = deferred<void>();
    vi.mocked(api.cancelWindowClose).mockReturnValue(cancellation.promise);
    const { user } = await renderLoaded(api);
    const first: WindowCloseRequest = {
      requestId: '1',
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    };
    const second = { ...first, requestId: '2' };

    closeCallback(api)(first);
    await user.click(await screen.findByRole('button', { name: 'Keep open' }));
    closeCallback(api)(second);
    cancellation.resolve();

    const dialog = await screen.findByRole('alertdialog', {
      name: 'Disconnect and close?',
    });
    await user.click(
      within(dialog).getByRole('button', { name: 'Disconnect and close' }),
    );
    expect(api.confirmWindowClose).toHaveBeenCalledWith('2');
  });

  it('does not let an older close confirmation rejection dismiss a newer request', async () => {
    const api = mockApi();
    const confirmation = deferred<void>();
    vi.mocked(api.confirmWindowClose).mockReturnValue(confirmation.promise);
    const { user } = await renderLoaded(api);
    const first: WindowCloseRequest = {
      requestId: '1',
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    };
    const second = { ...first, requestId: '2' };

    closeCallback(api)(first);
    await user.click(
      await screen.findByRole('button', { name: 'Disconnect and close' }),
    );
    closeCallback(api)(second);
    confirmation.reject({ kind: 'commandConflict' });

    const dialog = await screen.findByRole('alertdialog', {
      name: 'Disconnect and close?',
    });
    expect(
      within(dialog).getByRole('button', { name: 'Keep open' }),
    ).toBeEnabled();
    expect(
      screen.queryByText(/close request changed before it could be confirmed/i),
    ).toBeNull();
  });

  it('keeps a newer active close dialog when a stale editable runtime event arrives', async () => {
    const active = snapshot(primaryProfile, {
      revision: '20',
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
    const api = mockApi(active);
    await renderLoaded(api);
    closeCallback(api)({ requestId: '1', lifecycle: active.lifecycle });
    await screen.findByRole('alertdialog', { name: 'Disconnect and close?' });

    runtimeCallback(api)({
      kind: 'lifecycle',
      revision: '19',
      sessionId: '1',
      lifecycle: snapshot().lifecycle,
    });

    await waitFor(() =>
      expect(
        screen.getByRole('alertdialog', { name: 'Disconnect and close?' }),
      ).toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(screen.getByLabelText('Connection status')).toHaveTextContent(
        'Connected',
      ),
    );
  });

  it('dismisses a rejected close decision and requires a fresh native request', async () => {
    const api = mockApi();
    vi.mocked(api.confirmWindowClose).mockRejectedValue({
      kind: 'processLaunch',
    });
    const { user } = await renderLoaded(api);

    closeCallback(api)({
      requestId: '9',
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
    await user.click(
      await screen.findByRole('button', { name: 'Disconnect and close' }),
    );

    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull());
    expect(
      screen.getByText(/Use the window close control to continue/),
    ).toBeInTheDocument();
  });

  it('preserves the earliest loss boundary across incremental frontend eviction', async () => {
    const api = mockApi();
    await renderLoaded(api);
    const records = Array.from({ length: 2_000 }, (_, index) => ({
      sequence: String(index + 3),
      stream: 'stdout' as const,
      text: `record ${index + 3}`,
      classification: { kind: 'display' as const },
      truncated: false,
    }));

    runtimeCallback(api)({
      kind: 'bootstrap',
      revision: '13',
      sessionId: '1',
      lifecycle: snapshot().lifecycle,
      gap: { firstMissing: '1', nextAvailable: '3' },
      records,
    });
    runtimeCallback(api)({
      kind: 'output',
      revision: '14',
      sessionId: '1',
      lifecycle: snapshot().lifecycle,
      record: {
        sequence: '2003',
        stream: 'stdout',
        text: 'record 2003',
        classification: { kind: 'display' },
        truncated: false,
      },
    });

    const log = screen.getByRole('log', { name: 'Connection logs' });
    await waitFor(() =>
      expect(log).toHaveTextContent('Output unavailable: sequences 1–3.'),
    );
    expect(within(log).queryByText('record 3')).toBeNull();
    expect(within(log).getByText('record 2003')).toBeInTheDocument();
  });

  it('presents an actionable empty state without inventing persisted data', async () => {
    const { user } = await renderLoaded(mockApi(snapshot(null)));

    expect(screen.getByText('No profiles saved')).toBeInTheDocument();
    await user.click(
      screen.getByRole('button', { name: 'Add server profile' }),
    );

    expect(screen.getByLabelText('Profile name')).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Save profile' })).toBeEnabled();
    for (const field of [
      'Profile name',
      'Server IP or host',
      'Port',
      'Encryption key',
    ]) {
      expect(screen.getByLabelText(field)).toBeRequired();
    }
  });

  it('validates on blur and focuses the first invalid field on submit', async () => {
    const api = mockApi(snapshot(null));
    const { user } = await renderLoaded(api);
    await user.click(
      screen.getByRole('button', { name: 'Add server profile' }),
    );

    const name = screen.getByLabelText('Profile name');
    await user.click(screen.getByLabelText('Server IP or host'));
    expect(screen.getByText('Profile name is required.')).toBeInTheDocument();
    expect(name).toHaveAttribute('aria-invalid', 'true');

    await user.click(screen.getByRole('button', { name: 'Save profile' }));
    expect(name).toHaveFocus();
    expect(
      screen.getByText('Server IP or host is required.'),
    ).toBeInTheDocument();
    expect(screen.getByText('Port is required.')).toBeInTheDocument();
    expect(screen.getByText('Encryption key is required.')).toBeInTheDocument();
    expect(api.createProfile).not.toHaveBeenCalled();
  });

  it('creates a valid profile through the typed API', async () => {
    const created = snapshot(primaryProfile);
    const api = mockApi(snapshot(null));
    vi.mocked(api.createProfile).mockResolvedValue(created);
    const { user } = await renderLoaded(api);
    await user.click(
      screen.getByRole('button', { name: 'Add server profile' }),
    );

    await user.type(screen.getByLabelText('Profile name'), 'Primary');
    await user.type(
      screen.getByLabelText('Server IP or host'),
      '198.51.100.10',
    );
    await user.type(screen.getByLabelText('Port'), '9999');
    await user.type(screen.getByLabelText('Encryption key'), 'local-key');
    await user.click(screen.getByRole('button', { name: 'Save profile' }));

    const expectedDraft: ProfileDraft = {
      name: 'Primary',
      serverHost: '198.51.100.10',
      port: 9999,
      encryptionKey: 'local-key',
    };
    await waitFor(() =>
      expect(api.createProfile).toHaveBeenCalledWith(expectedDraft),
    );
    expect(screen.getByRole('button', { name: 'Edit' })).toBeEnabled();
    expect(
      screen.queryByRole('button', { name: 'Save profile' }),
    ).not.toBeInTheDocument();
  });

  it('preserves entries and associates backend validation with its field', async () => {
    const api = mockApi(snapshot(null));
    vi.mocked(api.createProfile).mockRejectedValue({
      kind: 'profileValidation',
      field: 'serverHost',
      issue: 'invalidFormat',
    });
    const { user } = await renderLoaded(api);
    await user.click(
      screen.getByRole('button', { name: 'Add server profile' }),
    );
    await user.type(screen.getByLabelText('Profile name'), 'Primary');
    await user.type(
      screen.getByLabelText('Server IP or host'),
      'server.example',
    );
    await user.type(screen.getByLabelText('Port'), '9999');
    await user.type(screen.getByLabelText('Encryption key'), 'preserved-key');

    await user.click(screen.getByRole('button', { name: 'Save profile' }));

    const host = screen.getByLabelText('Server IP or host');
    await waitFor(() => expect(host).toHaveFocus());
    expect(host).toHaveValue('server.example');
    expect(screen.getByLabelText('Encryption key')).toHaveValue(
      'preserved-key',
    );
    expect(
      screen.getByText('Enter a valid IPv4 address or host name.'),
    ).toBeInTheDocument();
  });

  it('updates the selected profile and applies the normalized backend snapshot', async () => {
    const normalizedProfile = {
      ...primaryProfile,
      name: 'Renamed',
      serverHost: 'server.example.com',
      port: 443,
    };
    const api = mockApi();
    vi.mocked(api.updateProfile).mockResolvedValue(snapshot(normalizedProfile));
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.clear(screen.getByLabelText('Profile name'));
    await user.type(screen.getByLabelText('Profile name'), ' Renamed ');
    await user.clear(screen.getByLabelText('Server IP or host'));
    await user.type(
      screen.getByLabelText('Server IP or host'),
      ' server.example.com ',
    );
    await user.clear(screen.getByLabelText('Port'));
    await user.type(screen.getByLabelText('Port'), '443');

    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() =>
      expect(api.updateProfile).toHaveBeenCalledWith(primaryProfile.id, {
        name: ' Renamed ',
        serverHost: ' server.example.com ',
        port: 443,
        encryptionKey: primaryProfile.encryptionKey,
      }),
    );
    expect(screen.getByLabelText('Profile name')).toHaveValue('Renamed');
    expect(screen.getByLabelText('Server IP or host')).toHaveValue(
      'server.example.com',
    );
  });

  it('preserves an existing profile draft when a duplicate name is rejected', async () => {
    const api = mockApi();
    vi.mocked(api.updateProfile).mockRejectedValue({
      kind: 'profileDuplicateName',
    });
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.clear(screen.getByLabelText('Profile name'));
    await user.type(screen.getByLabelText('Profile name'), 'Backup');

    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    const name = screen.getByLabelText('Profile name');
    await waitFor(() => expect(name).toHaveFocus());
    expect(name).toHaveValue('Backup');
    expect(
      screen.getByText('A profile with this name already exists.'),
    ).toBeInTheDocument();
  });

  it('requires confirmation before discarding edits for another selection', async () => {
    const api = mockApi();
    vi.mocked(api.selectProfile).mockResolvedValue(snapshot(backupProfile));
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.clear(screen.getByLabelText('Profile name'));
    await user.type(screen.getByLabelText('Profile name'), 'Changed');

    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Selected server profile' }),
      backupProfile.id,
    );

    const dialog = screen.getByRole('alertdialog', {
      name: 'Discard your changes?',
    });
    const discard = screen.getByRole('button', { name: 'Discard changes' });
    expect(dialog).toBeInTheDocument();
    expect(discard).toHaveFocus();
    expect(api.selectProfile).not.toHaveBeenCalled();

    await user.click(discard);
    await waitFor(() =>
      expect(api.selectProfile).toHaveBeenCalledWith(backupProfile.id),
    );
    expect(screen.getByLabelText('Profile name')).toHaveValue('Backup');
  });

  it('prevents Edit from replacing an active create or edit draft', async () => {
    const { user } = await renderLoaded();
    const edit = screen.getByRole('button', { name: 'Edit' });

    await user.click(screen.getByRole('button', { name: 'New' }));
    await user.type(screen.getByLabelText('Profile name'), 'Unsaved');
    expect(edit).toBeDisabled();
    expect(screen.getByLabelText('Profile name')).toHaveValue('Unsaved');

    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await user.click(edit);
    await user.clear(screen.getByLabelText('Profile name'));
    await user.type(screen.getByLabelText('Profile name'), 'Edited');
    expect(edit).toBeDisabled();
    expect(screen.getByLabelText('Profile name')).toHaveValue('Edited');
  });

  it('names destructive confirmation and traps keyboard focus until Escape', async () => {
    const api = mockApi();
    vi.mocked(api.deleteProfile).mockResolvedValue(snapshot(backupProfile));
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.click(screen.getByRole('button', { name: 'Delete profile' }));

    expect(
      screen.getByRole('alertdialog', { name: 'Delete “Primary”?' }),
    ).toBeInTheDocument();
    const confirmDelete = within(
      screen.getByRole('alertdialog', { name: 'Delete “Primary”?' }),
    ).getByRole('button', { name: 'Delete profile' });
    const cancel = screen.getByRole('button', { name: 'Cancel' });
    expect(confirmDelete).toHaveFocus();

    await user.tab();
    expect(cancel).toHaveFocus();
    await user.tab({ shift: true });
    expect(confirmDelete).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    expect(api.deleteProfile).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Delete profile' }),
      ).toHaveFocus(),
    );

    await user.click(screen.getByRole('button', { name: 'Delete profile' }));
    await user.click(
      screen.getAllByRole('button', { name: 'Delete profile' }).at(-1)!,
    );
    await waitFor(() =>
      expect(api.deleteProfile).toHaveBeenCalledWith(primaryProfile.id),
    );
  });

  it('uses standard keyboard activation for editor and disclosure controls', async () => {
    const { user } = await renderLoaded();
    const edit = screen.getByRole('button', { name: 'Edit' });
    edit.focus();
    await user.keyboard('{Enter}');
    expect(
      screen.getByRole('button', { name: 'Save changes' }),
    ).toBeInTheDocument();

    const advanced = screen.getByRole('button', { name: /Advanced/ });
    advanced.focus();
    await user.keyboard(' ');
    expect(advanced).toHaveAttribute('aria-expanded', 'true');
  });

  it('shows a retry path when the initial native snapshot is unavailable', async () => {
    const api = mockApi();
    vi.mocked(api.getAppSnapshot)
      .mockRejectedValueOnce(new Error('unavailable'))
      .mockResolvedValueOnce(snapshot());
    const user = userEvent.setup();
    render(App, { props: { api } });

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The application state could not be loaded.',
    );
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByLabelText('Profile name')).toHaveValue('Primary');
    expect(api.getAppSnapshot).toHaveBeenCalledTimes(2);
  });

  it('selects a native interface and displays every derived detail', async () => {
    const api = mockApi();
    vi.mocked(api.selectInterface).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '13',
        selectedInterfaceGuid: wifiInterface.guid,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Interface' }),
      wifiInterface.guid,
    );

    await waitFor(() =>
      expect(api.selectInterface).toHaveBeenCalledWith(wifiInterface.guid),
    );
    expect(screen.getByRole('combobox', { name: 'Interface' })).toHaveValue(
      wifiInterface.guid,
    );
    expect(screen.getByText(wifiInterface.interfaceName)).toBeInTheDocument();
    expect(screen.getByText(wifiInterface.guid)).toBeInTheDocument();
    expect(screen.getByText(wifiInterface.localAddress)).toBeInTheDocument();
    expect(screen.getByText(wifiInterface.gatewayAddress)).toBeInTheDocument();
    expect(screen.getByText(wifiInterface.gatewayMac)).toBeInTheDocument();
  });

  it('refreshes canonical interfaces without discarding an active profile draft', async () => {
    const refreshedEthernet = {
      ...ethernetInterface,
      localAddress: '192.0.2.44',
    };
    const api = mockApi();
    vi.mocked(api.refreshInterfaces).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '13',
        interfaces: [refreshedEthernet],
        selectedInterfaceGuid: refreshedEthernet.guid,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.clear(screen.getByLabelText('Profile name'));
    await user.type(screen.getByLabelText('Profile name'), 'Unsaved name');
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    await user.click(screen.getByRole('button', { name: 'Refresh' }));

    await waitFor(() => expect(api.refreshInterfaces).toHaveBeenCalledOnce());
    expect(screen.getByText('192.0.2.44')).toBeInTheDocument();
    expect(screen.getByLabelText('Profile name')).toHaveValue('Unsaved name');
    expect(
      screen.getByRole('button', { name: 'Save changes' }),
    ).toBeInTheDocument();
  });

  it('does not regress canonical state when an older interface snapshot arrives', async () => {
    const current = snapshot(primaryProfile, {
      revision: '9007199254740993',
    });
    const api = mockApi(current);
    vi.mocked(api.refreshInterfaces).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '9007199254740992',
        interfaces: [wifiInterface],
        selectedInterfaceGuid: wifiInterface.guid,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    await user.click(screen.getByRole('button', { name: 'Refresh' }));

    await waitFor(() => expect(api.refreshInterfaces).toHaveBeenCalledOnce());
    expect(screen.getByRole('combobox', { name: 'Interface' })).toHaveValue(
      ethernetInterface.guid,
    );
    expect(
      screen.getByText(ethernetInterface.localAddress),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(wifiInterface.gatewayMac),
    ).not.toBeInTheDocument();
  });

  it('serializes a pending interface mutation and preserves the profile draft', async () => {
    const pendingSelection = deferred<AppSnapshot>();
    const api = mockApi();
    vi.mocked(api.selectInterface).mockReturnValue(pendingSelection.promise);
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.clear(screen.getByLabelText('Profile name'));
    await user.type(screen.getByLabelText('Profile name'), 'Pending draft');
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Interface' }),
      wifiInterface.guid,
    );

    expect(await screen.findByRole('status')).toHaveTextContent(
      'Selecting network interface…',
    );
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeDisabled();
    expect(screen.getByRole('combobox', { name: 'Interface' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Delete profile' }),
    ).toBeDisabled();
    expect(
      screen.getByRole('combobox', { name: 'Selected server profile' }),
    ).toBeDisabled();
    expect(screen.getByLabelText('Profile name')).toHaveValue('Pending draft');
    expect(api.refreshInterfaces).not.toHaveBeenCalled();

    pendingSelection.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        selectedInterfaceGuid: wifiInterface.guid,
      }),
    );

    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Interface' })).toHaveValue(
        wifiInterface.guid,
      ),
    );
    expect(screen.queryByText('Selecting network interface…')).toBeNull();
    expect(screen.getByLabelText('Profile name')).toHaveValue('Pending draft');
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeEnabled();
  });

  it('distinguishes an empty interface result from refresh failure', async () => {
    const api = mockApi(
      snapshot(primaryProfile, {
        interfaces: [],
        selectedInterfaceGuid: null,
      }),
    );
    vi.mocked(api.refreshInterfaces).mockRejectedValue({
      kind: 'networkDiscovery',
    });
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    expect(screen.getByText('No usable interfaces found')).toBeInTheDocument();
    expect(
      screen.getByText('No usable network interface is available.'),
    ).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Windows network interfaces could not be refreshed.',
    );
  });

  it('keeps Advanced inspectable while all native mutation controls are locked', async () => {
    const locked = snapshot(primaryProfile, {
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
    const { user } = await renderLoaded(mockApi(locked));

    expect(screen.getByRole('button', { name: 'New' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Edit' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeDisabled();
    expect(screen.getByRole('combobox', { name: 'Interface' })).toBeDisabled();
    expect(screen.getByText(ethernetInterface.guid)).toBeInTheDocument();
  });

  it('maps a disappeared interface to an actionable native error', async () => {
    const api = mockApi();
    vi.mocked(api.selectInterface).mockRejectedValue({
      kind: 'interfaceNotFound',
    });
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Interface' }),
      wifiInterface.guid,
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'no longer available',
    );
    expect(screen.getByRole('combobox', { name: 'Interface' })).toHaveValue(
      ethernetInterface.guid,
    );
  });

  it('renders every common override off with its pinned starting value', async () => {
    const { user } = await renderLoaded();
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    expect(screen.getAllByRole('checkbox')).toHaveLength(16);
    expect(screen.getByRole('combobox', { name: 'Log level' })).toHaveValue(
      'info',
    );
    expect(screen.getByLabelText('PCAP socket buffer')).toHaveValue('4194304');
    expect(screen.getByLabelText('Local TCP flags')).toHaveValue('PA');
    expect(screen.getByLabelText('Remote TCP flags')).toHaveValue('PA');
    expect(screen.getByLabelText('Connection count')).toHaveValue('1');
    expect(screen.getByLabelText('TCP buffer')).toHaveValue('8192');
    expect(screen.getByLabelText('UDP buffer')).toHaveValue('4096');
    for (const control of [
      screen.getByRole('combobox', { name: 'Log level' }),
      screen.getByLabelText('PCAP socket buffer'),
      screen.getByLabelText('Local TCP flags'),
      screen.getByLabelText('Remote TCP flags'),
      screen.getByLabelText('Connection count'),
      screen.getByLabelText('TCP buffer'),
      screen.getByLabelText('UDP buffer'),
    ]) {
      expect(control).toBeDisabled();
    }
  });

  it('enables an override immediately while preserving untouched KCP and SMUX settings', async () => {
    const existing = advancedSettings({
      kcpMode: 'fast3',
      kcpMtu: 1400,
      smuxBuffer: 4_194_304,
    });
    const initial = snapshot(primaryProfile, { advancedSettings: existing });
    const updated = snapshot(primaryProfile, {
      revision: '13',
      advancedSettings: { ...existing, pcapSocketBuffer: 4_194_304 },
    });
    const api = mockApi(initial);
    vi.mocked(api.replaceAdvancedSettings).mockResolvedValue(updated);
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    await user.click(
      screen.getByRole('checkbox', {
        name: /Override PCAP socket buffer/,
      }),
    );

    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledWith({
        ...existing,
        pcapSocketBuffer: 4_194_304,
      }),
    );
    expect(screen.getByLabelText('PCAP socket buffer')).toBeEnabled();
    expect(screen.getByLabelText('PCAP socket buffer')).toHaveValue('4194304');
  });

  it('updates log level immediately and allows only info or debug', async () => {
    const infoSettings = advancedSettings({ logLevel: 'info' });
    const debugSettings = advancedSettings({ logLevel: 'debug' });
    const api = mockApi();
    vi.mocked(api.replaceAdvancedSettings)
      .mockResolvedValueOnce(
        snapshot(primaryProfile, {
          revision: '13',
          advancedSettings: infoSettings,
        }),
      )
      .mockResolvedValueOnce(
        snapshot(primaryProfile, {
          revision: '14',
          advancedSettings: debugSettings,
        }),
      );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    await user.click(
      screen.getByRole('checkbox', { name: /Override log level/ }),
    );
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Log level' }),
      'debug',
    );

    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenLastCalledWith(
        debugSettings,
      ),
    );
    expect(
      within(screen.getByRole('combobox', { name: 'Log level' })).getAllByRole(
        'option',
      ),
    ).toHaveLength(2);
    expect(screen.getByRole('combobox', { name: 'Log level' })).toHaveValue(
      'debug',
    );
  });

  it('commits a just-blurred Advanced draft before connecting', async () => {
    const initialSettings = advancedSettings({ connectionCount: 1 });
    const updatedSettings = advancedSettings({ connectionCount: 2 });
    const updated = snapshot(primaryProfile, {
      revision: '13',
      advancedSettings: updatedSettings,
    });
    const replacement = deferred<AppSnapshot>();
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockReturnValue(replacement.promise);
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    const count = screen.getByLabelText('Connection count');
    await user.clear(count);
    await user.type(count, '2');
    await user.click(screen.getByRole('button', { name: 'Connect' }));

    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(updatedSettings),
    );
    expect(api.connect).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        screen.getByRole('region', { name: 'Server profile' }),
      ).toHaveAttribute('aria-busy', 'true'),
    );
    expect(
      screen.getByRole('combobox', { name: 'Selected server profile' }),
    ).toBeDisabled();
    replacement.resolve(updated);
    await waitFor(() => expect(api.connect).toHaveBeenCalledOnce());
  });

  it('blocks Connect when a just-blurred enabled Advanced draft is invalid', async () => {
    const api = mockApi(
      snapshot(primaryProfile, {
        advancedSettings: advancedSettings({ connectionCount: 1 }),
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    const count = screen.getByLabelText('Connection count');
    await user.clear(count);
    await user.type(count, '0');
    await user.click(screen.getByRole('button', { name: 'Connect' }));

    expect(
      await screen.findByText(
        'Correct the invalid Advanced setting before connecting.',
      ),
    ).toBeInTheDocument();
    expect(api.connect).not.toHaveBeenCalled();
  });

  it('keeps invalid TCP flags local and commits ordered comma-separated combinations', async () => {
    const initialSettings = advancedSettings({ localTcpFlags: ['PA'] });
    const updatedSettings = advancedSettings({
      localTcpFlags: ['PA', 'S', 'PA'],
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: updatedSettings,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const flags = screen.getByLabelText('Local TCP flags');

    await user.clear(flags);
    await user.type(flags, 'pa');
    await user.tab();
    expect(api.replaceAdvancedSettings).not.toHaveBeenCalled();
    expect(flags).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByText(/uppercase combinations/)).toBeInTheDocument();

    await user.clear(flags);
    await user.type(flags, ' PA, S, PA ');
    await user.keyboard('{Enter}');
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(updatedSettings),
    );
    expect(flags).toHaveValue('PA, S, PA');
  });

  it('validates numeric bounds and preserves decimal strings above safe integer range', async () => {
    const initialSettings = advancedSettings({
      connectionCount: 1,
      tcpBuffer: '8192',
    });
    const preciseSettings = advancedSettings({
      connectionCount: 1,
      tcpBuffer: '9007199254740993',
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: preciseSettings,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    const count = screen.getByLabelText('Connection count');
    await user.clear(count);
    await user.type(count, '0');
    await user.tab();
    expect(api.replaceAdvancedSettings).not.toHaveBeenCalled();
    expect(count).toHaveAttribute('aria-invalid', 'true');
    expect(
      screen.getByText('Connection count must be between 1 and 256.'),
    ).toBeInTheDocument();

    const tcpBuffer = screen.getByLabelText('TCP buffer');
    await user.clear(tcpBuffer);
    await user.type(tcpBuffer, '9007199254740993');
    await user.keyboard('{Enter}');
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(preciseSettings),
    );
    expect(tcpBuffer).toHaveValue('9007199254740993');
  });

  it('serializes a pending settings replacement with all native mutations', async () => {
    const pendingSettings = deferred<AppSnapshot>();
    const api = mockApi();
    vi.mocked(api.replaceAdvancedSettings).mockReturnValue(
      pendingSettings.promise,
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    await user.click(
      screen.getByRole('checkbox', { name: /Override connection count/ }),
    );

    expect(await screen.findByRole('status')).toHaveTextContent(
      'Updating connection count…',
    );
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeDisabled();
    expect(
      screen.getByRole('combobox', { name: 'Selected server profile' }),
    ).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Edit' })).toBeDisabled();
    const overrideCheckboxes = screen.getAllByRole('checkbox');
    expect(
      overrideCheckboxes.filter((control) => control.hasAttribute('disabled')),
    ).toHaveLength(1);
    expect(
      screen.getByRole('checkbox', { name: /Override connection count/ }),
    ).toBeDisabled();

    pendingSettings.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: advancedSettings({ connectionCount: 1 }),
      }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText('Connection count')).toBeEnabled(),
    );
    expect(screen.queryByText('Updating connection count…')).toBeNull();
  });

  it('preserves the next click after a valid text field commits on blur', async () => {
    const initialSettings = advancedSettings({ tcpBuffer: '8192' });
    const logUpdated = advancedSettings({
      logLevel: 'info',
      tcpBuffer: '8192',
    });
    const bothUpdated = advancedSettings({
      logLevel: 'info',
      tcpBuffer: '16384',
    });
    const pendingLog = deferred<AppSnapshot>();
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings)
      .mockReturnValueOnce(pendingLog.promise)
      .mockResolvedValueOnce(
        snapshot(primaryProfile, {
          revision: '14',
          advancedSettings: bothUpdated,
        }),
      );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const tcpBuffer = screen.getByLabelText('TCP buffer');

    await user.clear(tcpBuffer);
    await user.type(tcpBuffer, '16384');
    await user.click(
      screen.getByRole('checkbox', { name: /Override log level/ }),
    );

    expect(api.replaceAdvancedSettings).toHaveBeenCalledWith({
      ...initialSettings,
      logLevel: 'info',
    });
    pendingLog.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: logUpdated,
      }),
    );
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledTimes(2),
    );
    expect(api.replaceAdvancedSettings).toHaveBeenLastCalledWith(bothUpdated);
    expect(tcpBuffer).toHaveValue('16384');
  });

  it('preserves an invalid unrelated draft when another override succeeds', async () => {
    const initialSettings = advancedSettings({ connectionCount: 1 });
    const updatedSettings = advancedSettings({
      logLevel: 'info',
      connectionCount: 1,
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: updatedSettings,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const count = screen.getByLabelText('Connection count');

    await user.clear(count);
    await user.type(count, '0');
    await user.tab();
    expect(
      await screen.findByText(/Connection count must be/),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole('checkbox', { name: /Override log level/ }),
    );

    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledOnce(),
    );
    expect(count).toHaveValue('0');
    expect(count).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByText(/Connection count must be/)).toBeInTheDocument();
  });

  it('keeps keyboard focus on the next Advanced control while a blur commit is pending', async () => {
    const pendingBuffer = deferred<AppSnapshot>();
    const initialSettings = advancedSettings({
      tcpBuffer: '8192',
      udpBuffer: '4096',
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockReturnValue(
      pendingBuffer.promise,
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const tcpBuffer = screen.getByLabelText('TCP buffer');
    const udpOverride = screen.getByRole('checkbox', {
      name: /Override UDP buffer/,
    });

    await user.clear(tcpBuffer);
    await user.type(tcpBuffer, '16384');
    tcpBuffer.focus();
    await user.tab();
    expect(udpOverride).toHaveFocus();
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledOnce(),
    );
    expect(udpOverride).toHaveFocus();
    expect(udpOverride).toBeEnabled();

    pendingBuffer.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: { ...initialSettings, tcpBuffer: '16384' },
      }),
    );
  });

  it('preserves a second edit to the same field while its first replacement is pending', async () => {
    const firstReplacement = deferred<AppSnapshot>();
    const initialSettings = advancedSettings({ tcpBuffer: '8192' });
    const firstSettings = advancedSettings({ tcpBuffer: '16384' });
    const secondSettings = advancedSettings({ tcpBuffer: '32768' });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings)
      .mockReturnValueOnce(firstReplacement.promise)
      .mockResolvedValueOnce(
        snapshot(primaryProfile, {
          revision: '14',
          advancedSettings: secondSettings,
        }),
      );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const buffer = screen.getByLabelText('TCP buffer');

    await user.clear(buffer);
    await user.type(buffer, '16384');
    await user.tab();
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(firstSettings),
    );
    await user.click(buffer);
    await user.clear(buffer);
    await user.type(buffer, '32768');
    await user.tab();
    expect(buffer).toHaveValue('32768');

    firstReplacement.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: firstSettings,
      }),
    );
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledTimes(2),
    );
    expect(api.replaceAdvancedSettings).toHaveBeenLastCalledWith(
      secondSettings,
    );
    expect(buffer).toHaveValue('32768');
  });

  it('retains a field error when disabling its override is rejected', async () => {
    const initialSettings = advancedSettings({ connectionCount: 1 });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockRejectedValue({
      kind: 'settingsLocked',
    });
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const count = screen.getByLabelText('Connection count');

    await user.clear(count);
    await user.type(count, '0');
    await user.tab();
    expect(
      await screen.findByText(/Connection count must be/),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole('checkbox', { name: /Override connection count/ }),
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Advanced settings are locked',
    );
    expect(count).toHaveValue('0');
    expect(count).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByText(/Connection count must be/)).toBeInTheDocument();
  });

  it('waits for a pending Advanced update before confirming profile deletion', async () => {
    const pendingSettings = deferred<AppSnapshot>();
    const api = mockApi(
      snapshot(primaryProfile, {
        advancedSettings: advancedSettings({ tcpBuffer: '8192' }),
      }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockReturnValue(
      pendingSettings.promise,
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const buffer = screen.getByLabelText('TCP buffer');
    await user.clear(buffer);
    await user.type(buffer, '16384');
    await user.click(screen.getByRole('button', { name: 'Delete profile' }));
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledOnce(),
    );

    await user.click(
      screen.getAllByRole('button', { name: 'Delete profile' }).at(-1)!,
    );
    expect(api.deleteProfile).not.toHaveBeenCalled();
    pendingSettings.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: advancedSettings({ tcpBuffer: '16384' }),
      }),
    );
    await waitFor(() =>
      expect(api.deleteProfile).toHaveBeenCalledWith(primaryProfile.id),
    );
  });

  it('keeps common overrides inspectable but locked and maps native lock failures', async () => {
    const api = mockApi();
    vi.mocked(api.replaceAdvancedSettings).mockRejectedValue({
      kind: 'settingsLocked',
    });
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    await user.click(
      screen.getByRole('checkbox', { name: /Override UDP buffer/ }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Advanced settings are locked while paqet is active.',
    );
    expect(screen.getByLabelText('UDP buffer')).toHaveValue('4096');
    expect(screen.getByLabelText('UDP buffer')).toBeDisabled();
  });

  it('shows canonical common values while lifecycle locking disables every override control', async () => {
    const locked = snapshot(primaryProfile, {
      advancedSettings: advancedSettings({
        logLevel: 'debug',
        localTcpFlags: ['PA', 'S'],
        tcpBuffer: '9007199254740993',
      }),
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
    const { user } = await renderLoaded(mockApi(locked));
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    expect(screen.getByRole('combobox', { name: 'Log level' })).toHaveValue(
      'debug',
    );
    expect(screen.getByLabelText('Local TCP flags')).toHaveValue('PA, S');
    expect(screen.getByLabelText('TCP buffer')).toHaveValue('9007199254740993');
    expect(
      screen
        .getAllByRole('checkbox')
        .every((control) => control.hasAttribute('disabled')),
    ).toBe(true);
    expect(screen.getByRole('combobox', { name: 'Log level' })).toBeDisabled();
    expect(screen.getByLabelText('Local TCP flags')).toBeDisabled();
    expect(screen.getByLabelText('TCP buffer')).toBeDisabled();
  });

  it('renders every KCP and SMUX override off with its pinned starting value', async () => {
    const { user } = await renderLoaded();
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    expect(screen.getByRole('combobox', { name: 'KCP mode' })).toHaveValue(
      'fast',
    );
    expect(screen.getByLabelText('KCP MTU')).toHaveValue('1350');
    expect(screen.getByLabelText('KCP receive window')).toHaveValue('512');
    expect(screen.getByLabelText('KCP send window')).toHaveValue('512');
    expect(screen.getByRole('combobox', { name: 'KCP block' })).toHaveValue(
      'aes',
    );
    expect(screen.getByLabelText('SMUX buffer')).toHaveValue('4194304');
    expect(screen.getByLabelText('Stream buffer')).toHaveValue('2097152');
    expect(screen.getByLabelText('SMUX keepalive')).toHaveValue('2');
    expect(screen.getByLabelText('SMUX timeout')).toHaveValue('8');
    for (const control of [
      screen.getByRole('combobox', { name: 'KCP mode' }),
      screen.getByLabelText('KCP MTU'),
      screen.getByLabelText('KCP receive window'),
      screen.getByLabelText('KCP send window'),
      screen.getByRole('combobox', { name: 'KCP block' }),
      screen.getByLabelText('SMUX buffer'),
      screen.getByLabelText('Stream buffer'),
      screen.getByLabelText('SMUX keepalive'),
      screen.getByLabelText('SMUX timeout'),
    ]) {
      expect(control).toBeDisabled();
    }
  });

  it('derives a complete Manual tuple from the effective preset and clears it on exit', async () => {
    const fast2Settings = advancedSettings({ kcpMode: 'fast2' });
    const manualSettings = advancedSettings({
      kcpMode: 'manual',
      manualKcp: {
        noDelay: 1,
        interval: 20,
        resend: 2,
        noCongestion: 1,
        writeDelay: false,
        ackNoDelay: true,
      },
    });
    const normalSettings = advancedSettings({ kcpMode: 'normal' });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: fast2Settings }),
    );
    vi.mocked(api.replaceAdvancedSettings)
      .mockResolvedValueOnce(
        snapshot(primaryProfile, {
          revision: '13',
          advancedSettings: manualSettings,
        }),
      )
      .mockResolvedValueOnce(
        snapshot(primaryProfile, {
          revision: '14',
          advancedSettings: normalSettings,
        }),
      );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    await user.selectOptions(
      screen.getByRole('combobox', { name: 'KCP mode' }),
      'manual',
    );
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(manualSettings),
    );
    expect(screen.getByLabelText('KCP nodelay')).toHaveValue('1');
    expect(screen.getByLabelText('KCP interval')).toHaveValue('20');
    expect(screen.getByLabelText('KCP resend')).toHaveValue('2');
    expect(screen.getByLabelText('KCP nocongestion')).toHaveValue('1');
    expect(screen.getByLabelText('KCP write delay')).not.toBeChecked();
    expect(screen.getByLabelText('KCP ACK nodelay')).toBeChecked();

    await user.selectOptions(
      screen.getByRole('combobox', { name: 'KCP mode' }),
      'normal',
    );
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenLastCalledWith(
        normalSettings,
      ),
    );
    expect(
      screen.queryByRole('group', { name: 'Manual KCP tuning' }),
    ).toBeNull();
  });

  it('commits valid nested Manual values and keeps invalid values local', async () => {
    const initialSettings = advancedSettings({
      kcpMode: 'manual',
      manualKcp: {
        noDelay: 0,
        interval: 30,
        resend: 2,
        noCongestion: 1,
        writeDelay: true,
        ackNoDelay: false,
      },
    });
    const updatedSettings = advancedSettings({
      ...initialSettings,
      manualKcp: { ...initialSettings.manualKcp, interval: 75 },
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: updatedSettings,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const interval = screen.getByLabelText('KCP interval');

    await user.clear(interval);
    await user.type(interval, '9');
    await user.tab();
    expect(api.replaceAdvancedSettings).not.toHaveBeenCalled();
    expect(interval).toHaveAttribute('aria-invalid', 'true');
    expect(
      screen.getByText('KCP interval must be between 10 and 5000.'),
    ).toBeInTheDocument();

    await user.clear(interval);
    await user.type(interval, '75');
    await user.keyboard('{Enter}');
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(updatedSettings),
    );
    expect(interval).toHaveValue('75');
  });

  it('validates effective SMUX buffer relationships before replacement', async () => {
    const initialSettings = advancedSettings({ streamBuffer: 2_097_152 });
    const enabledSettings = advancedSettings({
      smuxBuffer: 4_194_304,
      streamBuffer: 2_097_152,
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: enabledSettings,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    await user.click(
      screen.getByRole('checkbox', { name: /Override SMUX buffer/ }),
    );
    const smuxBuffer = screen.getByLabelText('SMUX buffer');
    await waitFor(() => expect(smuxBuffer).toBeEnabled());
    await user.clear(smuxBuffer);
    await user.type(smuxBuffer, '1048576');
    await user.tab();

    expect(api.replaceAdvancedSettings).toHaveBeenCalledTimes(1);
    expect(smuxBuffer).toHaveAttribute('aria-invalid', 'true');
    expect(
      screen.getByText(
        'SMUX buffer must be at least the effective stream buffer.',
      ),
    ).toBeInTheDocument();
  });

  it('accepts equal SMUX keepalive and timeout but rejects a shorter timeout', async () => {
    const initialSettings = advancedSettings({
      smuxKeepalive: 8,
      smuxTimeout: 8,
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const timeout = screen.getByLabelText('SMUX timeout');

    await user.clear(timeout);
    await user.type(timeout, '7');
    await user.tab();
    expect(api.replaceAdvancedSettings).not.toHaveBeenCalled();
    expect(
      screen.getByText(
        'SMUX timeout must be at least the effective keepalive.',
      ),
    ).toBeInTheDocument();

    await user.clear(timeout);
    await user.type(timeout, '8');
    await user.keyboard('{Enter}');
    expect(timeout).not.toHaveAttribute('aria-invalid');
    expect(api.replaceAdvancedSettings).not.toHaveBeenCalled();
  });

  it('requires confirmation before committing each wire-distinct insecure KCP block', async () => {
    const initialSettings = advancedSettings({ kcpBlock: 'aes' });
    const nullSettings = advancedSettings({ kcpBlock: 'null' });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockResolvedValue(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: nullSettings,
      }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const block = screen.getByRole('combobox', { name: 'KCP block' });

    await user.selectOptions(block, 'none');
    expect(screen.getByRole('alertdialog')).toHaveTextContent(
      'Traffic will not be encrypted or authenticated',
    );
    expect(screen.getByRole('alertdialog')).toHaveTextContent(
      'None and Null are not interchangeable',
    );
    expect(api.replaceAdvancedSettings).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(block).toHaveFocus();
    expect(block).toHaveValue('aes');

    await user.selectOptions(block, 'null');
    await user.click(
      screen.getByRole('button', { name: 'Use insecure block' }),
    );
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(nullSettings),
    );
    expect(block).toHaveValue('null');
    expect(block).toHaveFocus();
    expect(
      within(block)
        .getAllByRole('option')
        .map((option) => option.getAttribute('value')),
    ).toEqual([
      'aes',
      'aes-128-gcm',
      'aes-128',
      'aes-192',
      'salsa20',
      'blowfish',
      'twofish',
      'cast5',
      '3des',
      'tea',
      'xtea',
      'xor',
      'sm4',
      'none',
      'null',
    ]);
  });

  it.each([
    ['normal', 0, 40, 2, 1, true, false],
    ['fast', 0, 30, 2, 1, true, false],
    ['fast2', 1, 20, 2, 1, false, true],
    ['fast3', 1, 10, 2, 1, false, true],
  ] as const)(
    'derives the complete %s preset when entering Manual mode',
    async (
      mode,
      noDelay,
      interval,
      resend,
      noCongestion,
      writeDelay,
      ackNoDelay,
    ) => {
      const initialSettings = advancedSettings({ kcpMode: mode });
      const expected = advancedSettings({
        kcpMode: 'manual',
        manualKcp: {
          noDelay,
          interval,
          resend,
          noCongestion,
          writeDelay,
          ackNoDelay,
        },
      });
      const api = mockApi(
        snapshot(primaryProfile, { advancedSettings: initialSettings }),
      );
      vi.mocked(api.replaceAdvancedSettings).mockResolvedValue(
        snapshot(primaryProfile, {
          revision: '13',
          advancedSettings: expected,
        }),
      );
      const { user } = await renderLoaded(api);
      await user.click(screen.getByRole('button', { name: /Advanced/ }));

      await user.selectOptions(
        screen.getByRole('combobox', { name: 'KCP mode' }),
        'manual',
      );
      await waitFor(() =>
        expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(expected),
      );
    },
  );

  it('does not recreate Manual fields from a queued boolean after leaving Manual mode', async () => {
    const initialSettings = advancedSettings({
      kcpMode: 'manual',
      manualKcp: {
        noDelay: 0,
        interval: 30,
        resend: 2,
        noCongestion: 1,
        writeDelay: true,
        ackNoDelay: false,
      },
    });
    const normalSettings = advancedSettings({ kcpMode: 'normal' });
    const pendingMode = deferred<AppSnapshot>();
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockReturnValue(pendingMode.promise);
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const writeDelay = screen.getByLabelText('KCP write delay');
    const interval = screen.getByLabelText('KCP interval');

    await user.selectOptions(
      screen.getByRole('combobox', { name: 'KCP mode' }),
      'normal',
    );
    expect(writeDelay).toBeDisabled();
    expect(interval).toBeDisabled();
    await user.click(writeDelay);
    pendingMode.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: normalSettings,
      }),
    );

    await waitFor(() =>
      expect(screen.queryByLabelText('KCP write delay')).toBeNull(),
    );
    expect(api.replaceAdvancedSettings).toHaveBeenCalledOnce();
    expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(normalSettings);
  });

  it('rejects disabling an effective SMUX dependency without marking its valid value invalid', async () => {
    const initialSettings = advancedSettings({
      smuxBuffer: 8_388_608,
      streamBuffer: 6_291_456,
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const smuxBuffer = screen.getByLabelText('SMUX buffer');

    await user.click(
      screen.getByRole('checkbox', { name: /Override SMUX buffer/ }),
    );

    expect(api.replaceAdvancedSettings).not.toHaveBeenCalled();
    expect(smuxBuffer).not.toHaveAttribute('aria-invalid');
    const transportSection = screen
      .getByRole('heading', { name: 'KCP and SMUX overrides' })
      .closest('section')!;
    expect(within(transportSection).getByRole('alert')).toHaveTextContent(
      'SMUX buffer must be at least the effective stream buffer.',
    );
    expect(
      screen.getByRole('checkbox', { name: /Override SMUX buffer/ }),
    ).toBeChecked();
  });

  it('rebases a queued KCP replacement on the latest common settings snapshot', async () => {
    const pendingLog = deferred<AppSnapshot>();
    const initialSettings = advancedSettings();
    const logSettings = advancedSettings({ logLevel: 'info' });
    const combinedSettings = advancedSettings({
      logLevel: 'info',
      kcpMtu: 1350,
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings)
      .mockReturnValueOnce(pendingLog.promise)
      .mockResolvedValueOnce(
        snapshot(primaryProfile, {
          revision: '14',
          advancedSettings: combinedSettings,
        }),
      );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    await user.click(
      screen.getByRole('checkbox', { name: /Override log level/ }),
    );
    await user.click(
      screen.getByRole('checkbox', { name: /Override KCP MTU/ }),
    );
    expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(logSettings);

    pendingLog.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: logSettings,
      }),
    );
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledTimes(2),
    );
    expect(api.replaceAdvancedSettings).toHaveBeenLastCalledWith(
      combinedSettings,
    );
  });

  it('orders a newer secure block selection after a confirmed insecure replacement', async () => {
    const initialSettings = advancedSettings({ kcpBlock: 'aes' });
    const noneSettings = advancedSettings({ kcpBlock: 'none' });
    const secureSettings = advancedSettings({ kcpBlock: 'aes-128-gcm' });
    const pendingInsecure = deferred<AppSnapshot>();
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings)
      .mockReturnValueOnce(pendingInsecure.promise)
      .mockResolvedValueOnce(
        snapshot(primaryProfile, {
          revision: '14',
          advancedSettings: secureSettings,
        }),
      );
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const block = screen.getByRole('combobox', { name: 'KCP block' });

    await user.selectOptions(block, 'none');
    await user.click(
      screen.getByRole('button', { name: 'Use insecure block' }),
    );
    expect(block).toHaveFocus();
    expect(block).toBeDisabled();
    expect(api.replaceAdvancedSettings).toHaveBeenCalledWith(noneSettings);

    pendingInsecure.resolve(
      snapshot(primaryProfile, {
        revision: '13',
        advancedSettings: noneSettings,
      }),
    );
    await waitFor(() => expect(block).toBeEnabled());
    await user.selectOptions(block, 'aes-128-gcm');
    await waitFor(() =>
      expect(api.replaceAdvancedSettings).toHaveBeenCalledTimes(2),
    );
    expect(api.replaceAdvancedSettings).toHaveBeenLastCalledWith(
      secureSettings,
    );
    expect(block).toHaveValue('aes-128-gcm');
  });

  it('restores a rejected Manual boolean to its canonical checked state', async () => {
    const initialSettings = advancedSettings({
      kcpMode: 'manual',
      manualKcp: {
        noDelay: 0,
        interval: 30,
        resend: 2,
        noCongestion: 1,
        writeDelay: true,
        ackNoDelay: false,
      },
    });
    const api = mockApi(
      snapshot(primaryProfile, { advancedSettings: initialSettings }),
    );
    vi.mocked(api.replaceAdvancedSettings).mockRejectedValue({
      kind: 'settingsLocked',
    });
    const { user } = await renderLoaded(api);
    await user.click(screen.getByRole('button', { name: /Advanced/ }));
    const writeDelay = screen.getByLabelText('KCP write delay');

    await user.click(writeDelay);

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Advanced settings are locked while paqet is active.',
    );
    expect(writeDelay).toBeChecked();
  });

  it('shows canonical KCP and SMUX values while lifecycle locking disables their controls', async () => {
    const locked = snapshot(primaryProfile, {
      advancedSettings: advancedSettings({
        kcpMode: 'fast3',
        kcpMtu: 1400,
        kcpBlock: 'aes-128-gcm',
        smuxBuffer: 8_388_608,
        streamBuffer: 4_194_304,
        smuxKeepalive: 4,
        smuxTimeout: 12,
      }),
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
    const { user } = await renderLoaded(mockApi(locked));
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    expect(screen.getByRole('combobox', { name: 'KCP mode' })).toHaveValue(
      'fast3',
    );
    expect(screen.getByLabelText('KCP MTU')).toHaveValue('1400');
    expect(screen.getByRole('combobox', { name: 'KCP block' })).toHaveValue(
      'aes-128-gcm',
    );
    expect(screen.getByLabelText('SMUX buffer')).toHaveValue('8388608');
    expect(screen.getByLabelText('Stream buffer')).toHaveValue('4194304');
    expect(screen.getByLabelText('SMUX keepalive')).toHaveValue('4');
    expect(screen.getByLabelText('SMUX timeout')).toHaveValue('12');
    for (const control of screen.getAllByRole('checkbox')) {
      expect(control).toBeDisabled();
    }
    expect(screen.getByRole('combobox', { name: 'KCP mode' })).toBeDisabled();
    expect(screen.getByLabelText('KCP MTU')).toBeDisabled();
    expect(screen.getByRole('combobox', { name: 'KCP block' })).toBeDisabled();
    expect(screen.getByLabelText('SMUX timeout')).toBeDisabled();
  });

  it('keeps a complete Manual tuple inspectable while lifecycle locking disables it', async () => {
    const locked = snapshot(primaryProfile, {
      advancedSettings: advancedSettings({
        kcpMode: 'manual',
        manualKcp: {
          noDelay: 1,
          interval: 10,
          resend: 2,
          noCongestion: 1,
          writeDelay: false,
          ackNoDelay: true,
        },
      }),
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
    const { user } = await renderLoaded(mockApi(locked));
    await user.click(screen.getByRole('button', { name: /Advanced/ }));

    expect(screen.getByLabelText('KCP nodelay')).toHaveValue('1');
    expect(screen.getByLabelText('KCP interval')).toHaveValue('10');
    expect(screen.getByLabelText('KCP resend')).toHaveValue('2');
    expect(screen.getByLabelText('KCP nocongestion')).toHaveValue('1');
    expect(screen.getByLabelText('KCP write delay')).not.toBeChecked();
    expect(screen.getByLabelText('KCP ACK nodelay')).toBeChecked();
    for (const label of [
      'KCP nodelay',
      'KCP interval',
      'KCP resend',
      'KCP nocongestion',
      'KCP write delay',
      'KCP ACK nodelay',
    ]) {
      expect(screen.getByLabelText(label)).toBeDisabled();
    }
  });
});

describe('fixed-window responsive and preference contracts', () => {
  it('keeps narrow and zoomed layouts vertically reachable without horizontal scrolling', () => {
    expect(styles).toContain('@media (max-width: 360px)');
    expect(styles).toContain('@media (max-width: 280px)');
    expect(styles).toContain('@media (max-height: 640px)');
    expect(styles).not.toContain('min-resolution');
    expect(styles).not.toContain('min-width: 320px');
    expect(styles).toMatch(/body\s*{\s*overflow-y: auto;/);
    expect(styles).toMatch(/\.configuration\s*{[^}]*overflow-x: hidden;/s);
    expect(styles).toMatch(/\.profile-toolbar\s*{\s*flex-wrap: wrap;/);
    expect(styles).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(styles).toContain('overflow-wrap: anywhere');
    expect(styles).toContain('grid-template-columns: 104px minmax(0, 1fr)');
    expect(styles).toContain('max-height: calc(100dvh - 40px)');
  });

  it('removes nonessential motion and preserves visible boundaries in forced colors', () => {
    expect(styles).toContain('@media (prefers-reduced-motion: reduce)');
    expect(styles).toContain('transition-duration: 0.01ms !important');
    expect(styles).toContain('@media (forced-colors: active)');
    expect(styles).toContain('border-color: CanvasText');
    expect(styles).toContain('outline: 2px solid var(--accent)');
  });

  it('uses the locally bundled Nightlife Utility visual system', () => {
    expect(styles).toMatch(
      /@font-face\s*{[^}]*font-family: 'Space Grotesk';[^}]*font-style: normal;[^}]*font-weight: 300 700;[^}]*font-display: swap;[^}]*url\('\.\/assets\/fonts\/space-grotesk-latin-variable\.woff2'\)/s,
    );
    expect(styles).toMatch(
      /@font-face\s*{[^}]*font-family: 'JetBrains Mono';[^}]*font-style: normal;[^}]*font-weight: 400 700;[^}]*font-display: swap;[^}]*url\('\.\/assets\/fonts\/jetbrains-mono-latin-variable\.woff2'\)/s,
    );
    expect(styles).toContain('--ink: #151116');
    expect(styles).toContain('--accent: #ff8a4c');
    expect(styles).toContain('--connected: #b7f05a');
    expect(styles).toContain('--danger: #ff7a82');
    expect(styles).toContain('--control-border: #897180');
    expect(styles).toMatch(
      /\.connect-button\.disconnect-action:hover:not\(:disabled\)\s*{[^}]*background: var\(--danger-hover\);/s,
    );
    expect(styles).not.toContain('font-weight: 750');
    expect(styles).not.toContain('#64c8d3');
    expect(styles).not.toContain('#0e171d');
  });

  it('pins and bundles complete notices for the local font resources', () => {
    const spaceGroteskHash =
      '0640890476fc1198ab4de571fb658de443c4d85b66466ec09534a8737ab1ce9d';
    const jetBrainsMonoHash =
      '83c005d49d8a6a50474c73a5a36ac0468076e9c4a29da7bdb14995d80560a5be';

    expect(sha256(spaceGroteskPath)).toBe(spaceGroteskHash);
    expect(sha256(jetBrainsMonoPath)).toBe(jetBrainsMonoHash);
    expect(notices).toContain(spaceGroteskHash);
    expect(notices).toContain(jetBrainsMonoHash);
    expect(notices).toContain('fonts.gstatic.com/s/spacegrotesk/v22/');
    expect(notices).toContain('fonts.gstatic.com/s/jetbrainsmono/v24/');
    expect(spaceGroteskLicense).toContain(
      'Copyright 2020 The Space Grotesk Project Authors',
    );
    expect(jetBrainsMonoLicense).toContain(
      'Copyright 2020 The JetBrains Mono Project Authors',
    );
    for (const license of [spaceGroteskLicense, jetBrainsMonoLicense]) {
      expect(license).toContain('SIL OPEN FONT LICENSE Version 1.1');
      expect(license).toContain('PERMISSION & CONDITIONS');
      expect(license).toContain('TERMINATION');
      expect(license).toContain('DISCLAIMER');
    }
    expect(tauriConfig.bundle.resources).toEqual({
      '../THIRD_PARTY_NOTICES.md': 'licenses/THIRD_PARTY_NOTICES.md',
      '../src/assets/fonts/SPACE_GROTESK_OFL.txt':
        'licenses/fonts/SPACE_GROTESK_OFL.txt',
      '../src/assets/fonts/JETBRAINS_MONO_OFL.txt':
        'licenses/fonts/JETBRAINS_MONO_OFL.txt',
    });
  });
});
