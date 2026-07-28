import { Channel, invoke } from '@tauri-apps/api/core';
import snapshotFixture from '../../../src-tauri/tests/fixtures/ipc/app-snapshot.json';
import configErrorFixture from '../../../src-tauri/tests/fixtures/ipc/error-config-validation.json';
import errorFixture from '../../../src-tauri/tests/fixtures/ipc/error-profile-validation.json';
import bootstrapFixture from '../../../src-tauri/tests/fixtures/ipc/runtime-bootstrap.json';
import gapFixture from '../../../src-tauri/tests/fixtures/ipc/runtime-gap.json';
import outputFixture from '../../../src-tauri/tests/fixtures/ipc/runtime-output.json';
import closeRequestFixture from '../../../src-tauri/tests/fixtures/ipc/window-close-request.json';
import {
  connect,
  cancelWindowClose,
  confirmWindowClose,
  createProfile,
  deleteProfile,
  disconnect,
  getAppSnapshot,
  onWindowCloseRequested,
  refreshInterfaces,
  replaceAdvancedSettings,
  selectInterface,
  selectProfile,
  subscribeRuntimeEvents,
  updateProfile,
} from './index';
import type {
  AppSnapshot,
  IpcError,
  ProfileDraft,
  RuntimeEvent,
  WindowCloseRequest,
} from './types';

vi.mock('@tauri-apps/api/core', () => ({
  Channel: vi.fn(
    class MockChannel<T> {
      constructor(public onmessage: (event: T) => void) {}
    },
  ),
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

describe('Tauri API', () => {
  it('uses the exact allowlisted command and argument envelopes', async () => {
    await getAppSnapshot();
    await createProfile(draft);
    await updateProfile('profile-id', draft);
    await deleteProfile('profile-id');
    await selectProfile('profile-id');
    await refreshInterfaces();
    await selectInterface('interface-guid');
    await replaceAdvancedSettings(snapshot.advancedSettings);
    await connect();
    await disconnect();
    const onEvent = vi.fn();
    await subscribeRuntimeEvents(onEvent);
    const channel = (
      invokeMock.mock.calls.at(-1)?.[1] as Record<string, unknown>
    ).onEvent as Channel<RuntimeEvent>;
    await cancelWindowClose('9');
    await confirmWindowClose('9');
    const onCloseRequest = vi.fn();
    await onWindowCloseRequested(onCloseRequest);

    expect(invokeMock.mock.calls).toEqual([
      ['get_app_snapshot'],
      ['create_profile', { draft }],
      ['update_profile', { id: 'profile-id', draft }],
      ['delete_profile', { id: 'profile-id' }],
      ['select_profile', { id: 'profile-id' }],
      ['refresh_interfaces'],
      ['select_interface', { guid: 'interface-guid' }],
      ['replace_advanced_settings', { settings: snapshot.advancedSettings }],
      ['connect'],
      ['disconnect'],
      [
        'subscribe_runtime_events',
        { onEvent: expect.any(Channel) as Channel<RuntimeEvent> },
      ],
      ['cancel_window_close', { requestId: '9' }],
      ['confirm_window_close', { requestId: '9' }],
      [
        'subscribe_window_close_requests',
        { onRequest: expect.any(Channel) as Channel<WindowCloseRequest> },
      ],
    ]);
    channel.onmessage(outputFixture as RuntimeEvent);
    expect(onEvent).toHaveBeenCalledWith(outputFixture);
  });

  it('subscribes to the backend close-confirmation event payload', async () => {
    const onRequest = vi.fn();
    await onWindowCloseRequested(onRequest);
    const channel = (
      invokeMock.mock.calls.at(-1)?.[1] as Record<string, unknown>
    ).onRequest as Channel<WindowCloseRequest>;
    const closeRequest = closeRequestFixture as WindowCloseRequest;
    channel.onmessage(closeRequest);

    expect(onRequest).toHaveBeenCalledWith(closeRequest);
    expect(closeRequest).toEqual({
      requestId: '9',
      lifecycle: {
        status: 'connected',
        process: 'running',
        failure: null,
        settingsEditable: false,
      },
    });
  });

  it('ignores in-flight close requests from a replaced subscription', async () => {
    const first = vi.fn();
    const second = vi.fn();
    await onWindowCloseRequested(first);
    const firstChannel = (
      invokeMock.mock.calls.at(-1)?.[1] as Record<string, unknown>
    ).onRequest as Channel<WindowCloseRequest>;
    await onWindowCloseRequested(second);
    const secondChannel = (
      invokeMock.mock.calls.at(-1)?.[1] as Record<string, unknown>
    ).onRequest as Channel<WindowCloseRequest>;
    const closeRequest = closeRequestFixture as WindowCloseRequest;

    firstChannel.onmessage(closeRequest);
    secondChannel.onmessage(closeRequest);

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith(closeRequest);
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
    expect(configErrorFixture as IpcError).toEqual({
      kind: 'configValidation',
      field: 'streamBuffer',
      issue: 'invalidCombination',
    });
  });

  it('pins representative ordered runtime events shared with Rust', () => {
    const events = [
      bootstrapFixture,
      outputFixture,
      gapFixture,
    ] as RuntimeEvent[];

    expect(events.map((event) => event.kind)).toEqual([
      'bootstrap',
      'output',
      'gap',
    ]);
    expect(events[0]).toMatchObject({
      revision: '20',
      sessionId: '3',
      gap: { firstMissing: '1', nextAvailable: '7' },
      records: [{ sequence: '7' }],
    });
    expect(events[1]).toMatchObject({
      revision: '21',
      record: { sequence: '8', classification: { kind: 'connectionLost' } },
    });
    expect(events[2]).toMatchObject({
      firstMissing: '1',
      nextAvailable: '7',
      lifecycle: { status: 'connected' },
    });
  });

  it('ignores in-flight events from a replaced runtime subscription', async () => {
    const first = vi.fn();
    const second = vi.fn();
    await subscribeRuntimeEvents(first);
    const firstChannel = (
      invokeMock.mock.calls.at(-1)?.[1] as Record<string, unknown>
    ).onEvent as Channel<RuntimeEvent>;
    await subscribeRuntimeEvents(second);
    const secondChannel = (
      invokeMock.mock.calls.at(-1)?.[1] as Record<string, unknown>
    ).onEvent as Channel<RuntimeEvent>;

    firstChannel.onmessage(outputFixture as RuntimeEvent);
    secondChannel.onmessage(bootstrapFixture as RuntimeEvent);

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith(bootstrapFixture);
  });
});
