export type ProfileId = string;

export interface ProfileDraft {
  name: string;
  serverHost: string;
  port: number;
  encryptionKey: string;
}

export interface Profile extends ProfileDraft {
  id: ProfileId;
}

export interface ProfileSummary {
  id: ProfileId;
  name: string;
  serverHost: string;
  port: number;
}

export interface NetworkInterface {
  friendlyName: string;
  interfaceName: string;
  guid: string;
  localAddress: string;
  gatewayAddress: string;
  gatewayMac: string;
}

export type LogLevel = 'debug' | 'info';
export type KcpMode = 'normal' | 'fast' | 'fast2' | 'fast3' | 'manual';
export type KcpBlock =
  | 'aes'
  | 'aes-128'
  | 'aes-128-gcm'
  | 'aes-192'
  | 'salsa20'
  | 'blowfish'
  | 'twofish'
  | 'cast5'
  | '3des'
  | 'tea'
  | 'xtea'
  | 'xor'
  | 'sm4'
  | 'none'
  | 'null';

export interface ManualKcpSettings {
  noDelay: number | null;
  interval: number | null;
  resend: number | null;
  noCongestion: number | null;
  writeDelay: boolean | null;
  ackNoDelay: boolean | null;
}

export interface AdvancedSettings {
  logLevel: LogLevel | null;
  pcapSocketBuffer: number | null;
  localTcpFlags: string[] | null;
  remoteTcpFlags: string[] | null;
  connectionCount: number | null;
  tcpBuffer: string | null;
  udpBuffer: string | null;
  kcpMode: KcpMode | null;
  manualKcp: ManualKcpSettings;
  kcpMtu: number | null;
  kcpReceiveWindow: number | null;
  kcpSendWindow: number | null;
  kcpBlock: KcpBlock | null;
  smuxBuffer: number | null;
  streamBuffer: number | null;
  smuxKeepalive: number | null;
  smuxTimeout: number | null;
}

export type LifecycleStatus =
  'disconnected' | 'connecting' | 'connected' | 'disconnecting' | 'failed';
export type ProcessPresence = 'absent' | 'running';
export type FailureReason =
  | { kind: 'launchFailed' }
  | { kind: 'connectionLost' }
  | { kind: 'configurationRejected' }
  | { kind: 'clientFailed' }
  | { kind: 'unexpectedExit'; code: number | null };

export interface LifecycleSnapshot {
  status: LifecycleStatus;
  process: ProcessPresence;
  failure: FailureReason | null;
  settingsEditable: boolean;
}

export interface AppSnapshot {
  revision: string;
  profiles: ProfileSummary[];
  selectedProfile: Profile | null;
  interfaces: NetworkInterface[];
  selectedInterfaceGuid: string | null;
  advancedSettings: AdvancedSettings;
  lifecycle: LifecycleSnapshot;
}

export type ProfileFieldName = 'name' | 'serverHost' | 'port' | 'encryptionKey';
export type ValidationIssue =
  'required' | 'invalidFormat' | 'outOfRange' | 'containsControlCharacters';

export type IpcError =
  | { kind: 'settingsLocked' }
  | { kind: 'interfaceNotFound' }
  | {
      kind: 'profileValidation';
      field: ProfileFieldName;
      issue: ValidationIssue;
    }
  | { kind: 'profileDuplicateName' }
  | { kind: 'profileNotFound' }
  | { kind: 'profileDataUnsupported'; version: number }
  | { kind: 'profileDataInvalid' }
  | { kind: 'profileStorage' }
  | { kind: 'networkDiscovery' }
  | { kind: 'stateUnavailable' };
