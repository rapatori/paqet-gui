import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  AdvancedSettings,
  AppSnapshot,
  ProfileDraft,
  ProfileId,
  RuntimeEvent,
  WindowCloseRequest,
} from './types';

export type * from './types';

let runtimeSubscriptionGeneration = 0;
let closeSubscriptionGeneration = 0;

export function getAppSnapshot(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('get_app_snapshot');
}

export function createProfile(draft: ProfileDraft): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('create_profile', { draft });
}

export function updateProfile(
  id: ProfileId,
  draft: ProfileDraft,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('update_profile', { id, draft });
}

export function deleteProfile(id: ProfileId): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('delete_profile', { id });
}

export function selectProfile(id: ProfileId): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('select_profile', { id });
}

export function refreshInterfaces(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('refresh_interfaces');
}

export function selectInterface(guid: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('select_interface', { guid });
}

export function setSocksPort(port: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_socks_port', { port });
}

export function replaceAdvancedSettings(
  settings: AdvancedSettings,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('replace_advanced_settings', { settings });
}

export function connect(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('connect');
}

export function disconnect(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('disconnect');
}

export function subscribeRuntimeEvents(
  onEvent: (event: RuntimeEvent) => void,
): Promise<void> {
  const generation = ++runtimeSubscriptionGeneration;
  const channel = new Channel<RuntimeEvent>((event) => {
    if (generation === runtimeSubscriptionGeneration) {
      onEvent(event);
    }
  });
  return invoke<void>('subscribe_runtime_events', { onEvent: channel });
}

export function onWindowCloseRequested(
  onRequest: (request: WindowCloseRequest) => void,
): Promise<void> {
  const generation = ++closeSubscriptionGeneration;
  const channel = new Channel<WindowCloseRequest>((request) => {
    if (generation === closeSubscriptionGeneration) {
      onRequest(request);
    }
  });
  return invoke<void>('subscribe_window_close_requests', {
    onRequest: channel,
  });
}

export function cancelWindowClose(requestId: string): Promise<void> {
  return invoke<void>('cancel_window_close', { requestId });
}

export function confirmWindowClose(requestId: string): Promise<void> {
  return invoke<void>('confirm_window_close', { requestId });
}
