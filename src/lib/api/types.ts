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

export type OutputStream = 'stdout' | 'stderr';
export type FatalKind = 'configuration' | 'client';
export type LogClassification =
  | { kind: 'display' }
  | { kind: 'connected' }
  | { kind: 'connectionLost' }
  | { kind: 'fatal'; fatalKind: FatalKind }
  | { kind: 'shutdownRequested' };

export interface LogRecord {
  sequence: string;
  stream: OutputStream;
  text: string;
  classification: LogClassification;
  truncated: boolean;
}

export interface RuntimeGap {
  firstMissing: string;
  nextAvailable: string;
}

export type RuntimeEvent =
  | {
      kind: 'bootstrap';
      revision: string;
      sessionId: string | null;
      lifecycle: LifecycleSnapshot;
      gap: RuntimeGap | null;
      records: LogRecord[];
    }
  | {
      kind: 'lifecycle';
      revision: string;
      sessionId: string | null;
      lifecycle: LifecycleSnapshot;
    }
  | {
      kind: 'output';
      revision: string;
      sessionId: string;
      lifecycle: LifecycleSnapshot;
      record: LogRecord;
    }
  | {
      kind: 'gap';
      revision: string;
      sessionId: string;
      firstMissing: string;
      nextAvailable: string;
      lifecycle: LifecycleSnapshot;
    };

export interface AppSnapshot {
  revision: string;
  profiles: ProfileSummary[];
  selectedProfile: Profile | null;
  interfaces: NetworkInterface[];
  selectedInterfaceGuid: string | null;
  advancedSettings: AdvancedSettings;
  lifecycle: LifecycleSnapshot;
}

export interface WindowCloseRequest {
  requestId: string;
  lifecycle: LifecycleSnapshot;
}

export type ProfileFieldName = 'name' | 'serverHost' | 'port' | 'encryptionKey';
export type ValidationIssue =
  | 'required'
  | 'invalidFormat'
  | 'outOfRange'
  | 'containsControlCharacters'
  | 'invalidCombination';
export type ConfigFieldName =
  | 'interfaceName'
  | 'interfaceGuid'
  | 'localAddress'
  | 'gatewayMac'
  | 'serverAddress'
  | 'encryptionKey'
  | 'pcapSocketBuffer'
  | 'localTcpFlags'
  | 'remoteTcpFlags'
  | 'connectionCount'
  | 'tcpBuffer'
  | 'udpBuffer'
  | 'kcpMode'
  | 'kcpNoDelay'
  | 'kcpInterval'
  | 'kcpResend'
  | 'kcpNoCongestion'
  | 'kcpMtu'
  | 'kcpReceiveWindow'
  | 'kcpSendWindow'
  | 'smuxBuffer'
  | 'streamBuffer'
  | 'smuxKeepalive'
  | 'smuxTimeout';

export type IpcError =
  | { kind: 'settingsLocked' }
  | { kind: 'interfaceNotFound' }
  | { kind: 'profileNotSelected' }
  | { kind: 'interfaceNotSelected' }
  | { kind: 'commandConflict' }
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
  | {
      kind: 'configValidation';
      field: ConfigFieldName;
      issue: ValidationIssue;
    }
  | { kind: 'configGeneration' }
  | { kind: 'configStorage' }
  | { kind: 'processLaunch' }
  | { kind: 'runtimeSubscription' }
  | { kind: 'stateUnavailable' };
