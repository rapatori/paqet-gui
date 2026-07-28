import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { vi } from 'vitest';
import App, { type AppApi } from './App.svelte';
import type {
  AppSnapshot,
  NetworkInterface,
  Profile,
  ProfileDraft,
} from './lib/api';

const styles = readFileSync(join(process.cwd(), 'src', 'styles.css'), 'utf8');

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

function mockApi(initialSnapshot = snapshot()): AppApi {
  return {
    getAppSnapshot: vi.fn().mockResolvedValue(initialSnapshot),
    createProfile: vi.fn().mockResolvedValue(initialSnapshot),
    updateProfile: vi.fn().mockResolvedValue(initialSnapshot),
    deleteProfile: vi.fn().mockResolvedValue(initialSnapshot),
    selectProfile: vi.fn().mockResolvedValue(initialSnapshot),
    refreshInterfaces: vi.fn().mockResolvedValue(initialSnapshot),
    selectInterface: vi.fn().mockResolvedValue(initialSnapshot),
  };
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
    expect(screen.getByRole('button', { name: 'Connect' })).toBeDisabled();
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
    expect(styles).toContain('outline: 2px solid #64c8d3');
  });
});
