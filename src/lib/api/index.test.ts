import { invoke } from '@tauri-apps/api/core';
import snapshotFixture from '../../../src-tauri/tests/fixtures/ipc/app-snapshot.json';
import errorFixture from '../../../src-tauri/tests/fixtures/ipc/error-profile-validation.json';
import {
  createProfile,
  deleteProfile,
  getAppSnapshot,
  refreshInterfaces,
  replaceAdvancedSettings,
  selectInterface,
  selectProfile,
  updateProfile,
} from './index';
import type { AppSnapshot, IpcError, ProfileDraft } from './types';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
const snapshot = snapshotFixture as AppSnapshot;
const draft: ProfileDraft = {
  name: 'Primary',
  serverHost: '198.51.100.10',
  port: 9999,
  encryptionKey: 'representative-test-key',
};

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(snapshot);
});

describe('disconnected Tauri API', () => {
  it('uses the exact allowlisted command and argument envelopes', async () => {
    await getAppSnapshot();
    await createProfile(draft);
    await updateProfile('profile-id', draft);
    await deleteProfile('profile-id');
    await selectProfile('profile-id');
    await refreshInterfaces();
    await selectInterface('interface-guid');
    await replaceAdvancedSettings(snapshot.advancedSettings);

    expect(invokeMock.mock.calls).toEqual([
      ['get_app_snapshot'],
      ['create_profile', { draft }],
      ['update_profile', { id: 'profile-id', draft }],
      ['delete_profile', { id: 'profile-id' }],
      ['select_profile', { id: 'profile-id' }],
      ['refresh_interfaces'],
      ['select_interface', { guid: 'interface-guid' }],
      ['replace_advanced_settings', { settings: snapshot.advancedSettings }],
    ]);
  });

  it('pins representative snapshot details shared with Rust', () => {
    expect(snapshot.revision).toBe('12');
    expect(snapshot.profiles[0]).not.toHaveProperty('encryptionKey');
    expect(snapshot.selectedProfile?.encryptionKey).toBe(
      'representative-test-key',
    );
    expect(snapshot.advancedSettings.tcpBuffer).toBe('9007199254740993');
    expect(snapshot.lifecycle).toEqual({
      status: 'disconnected',
      process: 'absent',
      failure: null,
      settingsEditable: true,
    });
  });

  it('pins the representative structured error shared with Rust', () => {
    const error = errorFixture as IpcError;

    expect(error).toEqual({
      kind: 'profileValidation',
      field: 'serverHost',
      issue: 'invalidFormat',
    });
  });
});
