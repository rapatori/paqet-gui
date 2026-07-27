import { invoke } from '@tauri-apps/api/core';
import type {
  AdvancedSettings,
  AppSnapshot,
  ProfileDraft,
  ProfileId,
} from './types';

export type * from './types';

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

export function replaceAdvancedSettings(
  settings: AdvancedSettings,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('replace_advanced_settings', { settings });
}
