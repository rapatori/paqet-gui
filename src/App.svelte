<script lang="ts">
  import { onMount, tick } from 'svelte';
  import * as tauriApi from './lib/api';
  import type {
    AdvancedSettings,
    AppSnapshot,
    IpcError,
    KcpBlock,
    KcpMode,
    LifecycleSnapshot,
    LogRecord,
    LogLevel,
    ManualKcpSettings,
    NetworkInterface,
    Profile,
    ProfileDraft,
    ProfileFieldName,
    ProfileId,
    RuntimeEvent,
    WindowCloseRequest,
  } from './lib/api';

  export interface AppApi {
    getAppSnapshot(): Promise<AppSnapshot>;
    createProfile(draft: ProfileDraft): Promise<AppSnapshot>;
    updateProfile(id: ProfileId, draft: ProfileDraft): Promise<AppSnapshot>;
    deleteProfile(id: ProfileId): Promise<AppSnapshot>;
    selectProfile(id: ProfileId): Promise<AppSnapshot>;
    refreshInterfaces(): Promise<AppSnapshot>;
    selectInterface(guid: string): Promise<AppSnapshot>;
    replaceAdvancedSettings(settings: AdvancedSettings): Promise<AppSnapshot>;
    connect(): Promise<AppSnapshot>;
    disconnect(): Promise<AppSnapshot>;
    subscribeRuntimeEvents(
      onEvent: (event: RuntimeEvent) => void,
    ): Promise<void>;
    onWindowCloseRequested(
      onRequest: (request: WindowCloseRequest) => void,
    ): Promise<void>;
    cancelWindowClose(requestId: string): Promise<void>;
    confirmWindowClose(requestId: string): Promise<void>;
  }

  type EditorMode = 'view' | 'create' | 'edit';
  type InterfaceOperation = 'refresh' | 'select' | null;
  type CommonSettingField =
    | 'logLevel'
    | 'pcapSocketBuffer'
    | 'localTcpFlags'
    | 'remoteTcpFlags'
    | 'connectionCount'
    | 'tcpBuffer'
    | 'udpBuffer';
  type CommonTextField = Exclude<CommonSettingField, 'logLevel'>;
  type CommonDraft = Record<CommonTextField, string>;
  type CommonErrors = Partial<Record<CommonTextField, string>>;
  type KcpSettingField =
    | 'kcpMode'
    | 'kcpNoDelay'
    | 'kcpInterval'
    | 'kcpResend'
    | 'kcpNoCongestion'
    | 'kcpWriteDelay'
    | 'kcpAckNoDelay'
    | 'kcpMtu'
    | 'kcpReceiveWindow'
    | 'kcpSendWindow'
    | 'kcpBlock'
    | 'smuxBuffer'
    | 'streamBuffer'
    | 'smuxKeepalive'
    | 'smuxTimeout';
  type KcpTextField = Exclude<
    KcpSettingField,
    'kcpMode' | 'kcpWriteDelay' | 'kcpAckNoDelay' | 'kcpBlock'
  >;
  type IndependentKcpField =
    | 'kcpMode'
    | 'kcpMtu'
    | 'kcpReceiveWindow'
    | 'kcpSendWindow'
    | 'kcpBlock'
    | 'smuxBuffer'
    | 'streamBuffer'
    | 'smuxKeepalive'
    | 'smuxTimeout';
  type IndependentKcpTextField = Exclude<
    IndependentKcpField,
    'kcpMode' | 'kcpBlock'
  >;
  type SettingsField = CommonSettingField | KcpSettingField;
  type KcpDraft = Record<KcpTextField, string>;
  type KcpErrors = Partial<Record<KcpTextField, string>>;
  type ProfileInput = Omit<ProfileDraft, 'port'> & { port: string };
  type FieldErrors = Partial<Record<ProfileFieldName, string>>;
  type LogEntry =
    | { kind: 'record'; sessionId: string; record: LogRecord }
    | {
        kind: 'gap';
        sessionId: string;
        firstMissing: string;
        nextAvailable: string;
      };
  type DialogState =
    | { kind: 'discardSelection'; profileId: ProfileId }
    | { kind: 'discardCreate' }
    | { kind: 'delete'; profile: Profile }
    | { kind: 'unsafeBlock'; value: 'none' | 'null' }
    | { kind: 'windowClose'; request: WindowCloseRequest }
    | null;

  const maxVisibleLogRecords = 2_000;
  const maxVisibleLogBytes = 512 * 1_024;
  const maxVisibleLogEntries = maxVisibleLogRecords * 2 + 1;
  const logBottomTolerance = 4;

  let { api = tauriApi }: { api?: AppApi } = $props();

  let snapshot = $state<AppSnapshot | null>(null);
  let editorMode = $state<EditorMode>('view');
  let draft = $state<ProfileInput>(emptyProfileInput());
  let fieldErrors = $state<FieldErrors>({});
  let message = $state('');
  let loading = $state(true);
  let saving = $state(false);
  let interfaceOperation = $state<InterfaceOperation>(null);
  let interfaceMessage = $state('');
  let settingsOperation = $state<SettingsField | null>(null);
  let kcpModeQueued = $state(false);
  let settingsMessage = $state('');
  let transportMessage = $state('');
  let commonDraft = $state<CommonDraft>(defaultCommonDraft());
  let commonErrors = $state<CommonErrors>({});
  let commonDraftVersions = $state<Record<CommonTextField, number>>(
    initialCommonDraftVersions(),
  );
  let kcpDraft = $state<KcpDraft>(defaultKcpDraft());
  let kcpErrors = $state<KcpErrors>({});
  let kcpDraftVersions = $state<Record<KcpTextField, number>>(
    initialKcpDraftVersions(),
  );
  let revealKey = $state(false);
  let advancedExpanded = $state(false);
  let dialog = $state<DialogState>(null);
  let runtimeReady = $state(false);
  let closeReady = $state(false);
  let connectionBusy = $state(false);
  let connectionMessage = $state('');
  let logEntries = $state<LogEntry[]>([]);
  let logSessionId = $state<string | null>(null);
  let followingLogs = $state(true);
  let copyMessage = $state('');
  let copyFailed = $state(false);
  let closeDecisionBusy = $state(false);
  let closeDecisionMessage = $state('');
  let lastRuntimeRevision: string | null = null;
  let pendingLifecycle: {
    revision: string;
    lifecycle: LifecycleSnapshot;
  } | null = null;

  let nameInput = $state<HTMLInputElement>();
  let serverHostInput = $state<HTMLInputElement>();
  let portInput = $state<HTMLInputElement>();
  let encryptionKeyInput = $state<HTMLInputElement>();
  let commonFieldInputs: Partial<Record<CommonTextField, HTMLInputElement>> =
    {};
  let kcpFieldInputs: Partial<Record<KcpTextField, HTMLInputElement>> = {};
  let dialogPrimaryButton = $state<HTMLButtonElement>();
  let dialogCancelButton = $state<HTMLButtonElement>();
  let dialogElement = $state<HTMLDivElement>();
  let profileSelect = $state<HTMLSelectElement>();
  let newProfileButton = $state<HTMLButtonElement>();
  let logElement = $state<HTMLDivElement>();
  let dialogInvoker: HTMLElement | null = null;
  let mutationIdleResolvers: Array<() => void> = [];
  let settingsQueue = Promise.resolve();
  let scheduledDraftCommits = Promise.resolve();

  const selectedProfile = $derived(snapshot?.selectedProfile ?? null);
  const selectedInterface = $derived(
    snapshot?.interfaces.find(
      (networkInterface) =>
        networkInterface.guid === snapshot?.selectedInterfaceGuid,
    ) ?? null,
  );
  const settingsEditable = $derived(
    snapshot?.lifecycle.settingsEditable ?? false,
  );
  const editorOpen = $derived(editorMode !== 'view');
  const interfaceBusy = $derived(interfaceOperation !== null);
  const settingsBusy = $derived(settingsOperation !== null);
  const mutationBusy = $derived(saving || interfaceBusy || settingsBusy);
  const draftChanged = $derived(
    editorMode === 'create'
      ? Object.values(draft).some((value) => value.length > 0)
      : editorMode === 'edit' && selectedProfile
        ? !profileInputMatches(draft, selectedProfile)
        : false,
  );
  const statusLabel = $derived(
    snapshot ? formatStatus(snapshot.lifecycle.status) : 'Disconnected',
  );
  const failureMessage = $derived(
    snapshot?.lifecycle.failure
      ? formatFailure(snapshot.lifecycle.failure)
      : '',
  );
  const connectionAction = $derived.by(() => {
    const lifecycle = snapshot?.lifecycle;
    if (!lifecycle) return { label: 'Connect', kind: 'connect' as const };
    if (lifecycle.status === 'connecting') {
      return { label: 'Connecting…', kind: 'waiting' as const };
    }
    if (lifecycle.status === 'disconnecting') {
      return { label: 'Disconnecting…', kind: 'waiting' as const };
    }
    if (lifecycle.process === 'running') {
      return { label: 'Disconnect', kind: 'disconnect' as const };
    }
    return { label: 'Connect', kind: 'connect' as const };
  });
  const connectionDisabled = $derived(
    loading ||
      !snapshot ||
      !runtimeReady ||
      connectionBusy ||
      connectionAction.kind === 'waiting' ||
      (connectionAction.kind === 'connect' &&
        (!closeReady ||
          mutationBusy ||
          editorOpen ||
          !selectedProfile ||
          !selectedInterface)),
  );

  onMount(() => {
    void subscribeRuntime();
    void subscribeWindowClose();
    void loadSnapshot();
  });

  function emptyProfileInput(): ProfileInput {
    return { name: '', serverHost: '', port: '', encryptionKey: '' };
  }

  function defaultCommonDraft(): CommonDraft {
    return {
      pcapSocketBuffer: '4194304',
      localTcpFlags: 'PA',
      remoteTcpFlags: 'PA',
      connectionCount: '1',
      tcpBuffer: '8192',
      udpBuffer: '4096',
    };
  }

  function initialCommonDraftVersions(): Record<CommonTextField, number> {
    return {
      pcapSocketBuffer: 0,
      localTcpFlags: 0,
      remoteTcpFlags: 0,
      connectionCount: 0,
      tcpBuffer: 0,
      udpBuffer: 0,
    };
  }

  function defaultKcpDraft(): KcpDraft {
    return {
      kcpNoDelay: '0',
      kcpInterval: '30',
      kcpResend: '2',
      kcpNoCongestion: '1',
      kcpMtu: '1350',
      kcpReceiveWindow: '512',
      kcpSendWindow: '512',
      smuxBuffer: '4194304',
      streamBuffer: '2097152',
      smuxKeepalive: '2',
      smuxTimeout: '8',
    };
  }

  function initialKcpDraftVersions(): Record<KcpTextField, number> {
    return {
      kcpNoDelay: 0,
      kcpInterval: 0,
      kcpResend: 0,
      kcpNoCongestion: 0,
      kcpMtu: 0,
      kcpReceiveWindow: 0,
      kcpSendWindow: 0,
      smuxBuffer: 0,
      streamBuffer: 0,
      smuxKeepalive: 0,
      smuxTimeout: 0,
    };
  }

  function profileInput(profile: Profile): ProfileInput {
    return {
      name: profile.name,
      serverHost: profile.serverHost,
      port: String(profile.port),
      encryptionKey: profile.encryptionKey,
    };
  }

  function profileInputMatches(input: ProfileInput, profile: Profile): boolean {
    return (
      input.name === profile.name &&
      input.serverHost === profile.serverHost &&
      input.port === String(profile.port) &&
      input.encryptionKey === profile.encryptionKey
    );
  }

  async function loadSnapshot(): Promise<void> {
    loading = true;
    message = '';
    try {
      applySnapshot(await api.getAppSnapshot(), true, 'all');
    } catch {
      message =
        'The application state could not be loaded. Restart PaqetGUI and try again.';
    } finally {
      loading = false;
    }
  }

  function applySnapshot(
    nextSnapshot: AppSnapshot,
    resetProfileEditor = false,
    fieldToSync: SettingsField | 'all' | null = null,
  ): boolean {
    if (snapshot && BigInt(nextSnapshot.revision) < BigInt(snapshot.revision)) {
      return false;
    }

    if (
      pendingLifecycle &&
      BigInt(pendingLifecycle.revision) > BigInt(nextSnapshot.revision)
    ) {
      nextSnapshot = {
        ...nextSnapshot,
        revision: pendingLifecycle.revision,
        lifecycle: pendingLifecycle.lifecycle,
      };
    }

    snapshot = nextSnapshot;
    if (resetProfileEditor) {
      editorMode = 'view';
      fieldErrors = {};
      revealKey = false;
      draft = nextSnapshot.selectedProfile
        ? profileInput(nextSnapshot.selectedProfile)
        : emptyProfileInput();
    }
    if (fieldToSync) {
      if (fieldToSync === 'all' || isCommonSettingField(fieldToSync)) {
        syncCommonDraft(nextSnapshot.advancedSettings, fieldToSync);
      }
      if (fieldToSync === 'all' || isKcpSettingField(fieldToSync)) {
        syncKcpDraft(nextSnapshot.advancedSettings, fieldToSync);
      }
    }
    return true;
  }

  async function subscribeRuntime(): Promise<void> {
    try {
      await api.subscribeRuntimeEvents(handleRuntimeEvent);
      runtimeReady = true;
    } catch {
      runtimeReady = false;
      connectionMessage =
        'Live connection state is unavailable. Restart PaqetGUI and try again.';
    }
  }

  async function subscribeWindowClose(): Promise<void> {
    try {
      await api.onWindowCloseRequested(handleWindowCloseRequest);
      closeReady = true;
    } catch {
      closeReady = false;
      connectionMessage =
        'Window-close confirmation is unavailable. Restart PaqetGUI and try again.';
    }
  }

  function handleRuntimeEvent(event: RuntimeEvent): void {
    if (
      lastRuntimeRevision !== null &&
      BigInt(event.revision) < BigInt(lastRuntimeRevision)
    ) {
      return;
    }
    lastRuntimeRevision = event.revision;
    const lifecycleApplied = applyRuntimeLifecycle(
      event.revision,
      event.lifecycle,
    );

    if (event.kind === 'bootstrap') {
      replaceLogSession(event.sessionId, event.records, event.gap);
    } else if (event.kind === 'output') {
      appendLogRecord(event.sessionId, event.record);
    } else if (event.kind === 'gap') {
      appendLogGap(event.sessionId, event.firstMissing, event.nextAvailable);
    } else if (event.sessionId !== null) {
      adoptLogSession(event.sessionId);
    }

    if (
      lifecycleApplied &&
      dialog?.kind === 'windowClose' &&
      snapshot?.lifecycle.settingsEditable
    ) {
      dismissWindowCloseDialog();
    }
  }

  function applyRuntimeLifecycle(
    revision: string,
    lifecycle: LifecycleSnapshot,
  ): boolean {
    pendingLifecycle = { revision, lifecycle };
    if (!snapshot) return true;
    if (BigInt(revision) < BigInt(snapshot.revision)) return false;
    snapshot = { ...snapshot, revision, lifecycle };
    return true;
  }

  function replaceLogSession(
    sessionId: string | null,
    records: LogRecord[],
    gap: { firstMissing: string; nextAvailable: string } | null,
  ): void {
    logSessionId = sessionId;
    if (sessionId === null) {
      logEntries = [];
      followingLogs = true;
      return;
    }

    const sortedRecords = [...records].sort((left, right) =>
      compareDecimal(left.sequence, right.sequence),
    );
    const entries: LogEntry[] = [];
    if (gap) {
      entries.push({ kind: 'gap', sessionId, ...gap });
    }
    let previousSequence: string | null = null;
    for (const record of sortedRecords) {
      if (
        previousSequence !== null &&
        BigInt(record.sequence) > BigInt(previousSequence) + 1n
      ) {
        entries.push({
          kind: 'gap',
          sessionId,
          firstMissing: (BigInt(previousSequence) + 1n).toString(),
          nextAvailable: record.sequence,
        });
      }
      if (previousSequence !== record.sequence) {
        entries.push({ kind: 'record', sessionId, record });
      }
      previousSequence = record.sequence;
    }
    logEntries = boundLogEntries(entries, sessionId);
    followingLogs = true;
    void scrollLogToLatest();
  }

  function adoptLogSession(sessionId: string): void {
    if (logSessionId === sessionId) return;
    logSessionId = sessionId;
    logEntries = [];
    followingLogs = true;
    copyMessage = '';
  }

  function appendLogRecord(sessionId: string, record: LogRecord): void {
    adoptLogSession(sessionId);
    if (
      logEntries.some(
        (entry) =>
          entry.kind === 'record' && entry.record.sequence === record.sequence,
      )
    ) {
      return;
    }
    const shouldFollow = followingLogs && isLogAtBottom();
    logEntries = boundLogEntries(
      [...logEntries, { kind: 'record', sessionId, record }],
      sessionId,
    );
    if (shouldFollow) void scrollLogToLatest();
  }

  function appendLogGap(
    sessionId: string,
    firstMissing: string,
    nextAvailable: string,
  ): void {
    adoptLogSession(sessionId);
    const duplicate = logEntries.some(
      (entry) =>
        entry.kind === 'gap' &&
        entry.firstMissing === firstMissing &&
        entry.nextAvailable === nextAvailable,
    );
    if (duplicate) return;
    const shouldFollow = followingLogs && isLogAtBottom();
    logEntries = boundLogEntries(
      [...logEntries, { kind: 'gap', sessionId, firstMissing, nextAvailable }],
      sessionId,
    );
    if (shouldFollow) void scrollLogToLatest();
  }

  function boundLogEntries(entries: LogEntry[], sessionId: string): LogEntry[] {
    const bounded = mergeAdjacentGaps(entries, sessionId);
    let recordCount = bounded.filter((entry) => entry.kind === 'record').length;
    let byteCount = bounded.reduce(
      (total, entry) =>
        total +
        (entry.kind === 'record' ? utf8ByteLength(entry.record.text) : 0),
      0,
    );
    let firstRemoved: string | null = null;
    let nextAvailable: string | null = null;

    while (
      recordCount > maxVisibleLogRecords ||
      byteCount > maxVisibleLogBytes
    ) {
      const index = bounded.findIndex((entry) => entry.kind === 'record');
      if (index === -1) break;
      const [removed] = bounded.splice(index, 1);
      if (removed.kind !== 'record') continue;
      firstRemoved ??= removed.record.sequence;
      recordCount -= 1;
      byteCount -= utf8ByteLength(removed.record.text);
      nextAvailable =
        bounded.find((entry) => entry.kind === 'record')?.record.sequence ??
        null;
    }

    if (firstRemoved && nextAvailable) {
      const firstRecordIndex = bounded.findIndex(
        (entry) => entry.kind === 'record',
      );
      const prefix = bounded.splice(0, firstRecordIndex);
      const earliestMissing = prefix.reduce(
        (earliest, entry) =>
          entry.kind === 'gap' && BigInt(entry.firstMissing) < BigInt(earliest)
            ? entry.firstMissing
            : earliest,
        firstRemoved,
      );
      const retentionGap: LogEntry = {
        kind: 'gap',
        sessionId,
        firstMissing: earliestMissing,
        nextAvailable,
      };
      bounded.unshift(retentionGap);
    }

    if (bounded.length > maxVisibleLogEntries) {
      const removeCount = bounded.length - maxVisibleLogEntries + 1;
      const removed = bounded.splice(0, removeCount);
      const firstRemaining = bounded[0];
      if (firstRemaining) {
        const firstMissing = removed.reduce((earliest, entry) => {
          const sequence =
            entry.kind === 'gap' ? entry.firstMissing : entry.record.sequence;
          return BigInt(sequence) < BigInt(earliest) ? sequence : earliest;
        }, firstEntrySequence(removed[0]));
        const nextAvailable = firstEntrySequence(firstRemaining);
        if (firstRemaining.kind === 'gap') bounded.shift();
        bounded.unshift({
          kind: 'gap',
          sessionId,
          firstMissing,
          nextAvailable,
        });
      }
    }
    return bounded;
  }

  function mergeAdjacentGaps(
    entries: LogEntry[],
    sessionId: string,
  ): LogEntry[] {
    const merged: LogEntry[] = [];
    for (const entry of entries) {
      const previous = merged.at(-1);
      if (
        entry.kind === 'gap' &&
        previous?.kind === 'gap' &&
        BigInt(entry.firstMissing) <= BigInt(previous.nextAvailable)
      ) {
        previous.nextAvailable =
          BigInt(entry.nextAvailable) > BigInt(previous.nextAvailable)
            ? entry.nextAvailable
            : previous.nextAvailable;
      } else {
        merged.push(entry.kind === 'gap' ? { ...entry, sessionId } : entry);
      }
    }
    return merged;
  }

  function firstEntrySequence(entry: LogEntry): string {
    return entry.kind === 'gap' ? entry.firstMissing : entry.record.sequence;
  }

  function utf8ByteLength(value: string): number {
    return new TextEncoder().encode(value).length;
  }

  function compareDecimal(left: string, right: string): number {
    const leftValue = BigInt(left);
    const rightValue = BigInt(right);
    return leftValue < rightValue ? -1 : leftValue > rightValue ? 1 : 0;
  }

  function syncCommonDraft(
    settings: AdvancedSettings,
    field: CommonSettingField | 'all',
  ): void {
    const defaults = defaultCommonDraft();
    const canonicalDraft: CommonDraft = {
      pcapSocketBuffer: String(
        settings.pcapSocketBuffer ?? defaults.pcapSocketBuffer,
      ),
      localTcpFlags:
        settings.localTcpFlags?.join(', ') ?? defaults.localTcpFlags,
      remoteTcpFlags:
        settings.remoteTcpFlags?.join(', ') ?? defaults.remoteTcpFlags,
      connectionCount: String(
        settings.connectionCount ?? defaults.connectionCount,
      ),
      tcpBuffer: settings.tcpBuffer ?? defaults.tcpBuffer,
      udpBuffer: settings.udpBuffer ?? defaults.udpBuffer,
    };
    if (field === 'all') {
      commonDraft = canonicalDraft;
      commonErrors = {};
      commonDraftVersions = initialCommonDraftVersions();
    } else if (field !== 'logLevel') {
      commonDraft[field] = canonicalDraft[field];
      delete commonErrors[field];
    }
  }

  function syncKcpDraft(
    settings: AdvancedSettings,
    field: KcpSettingField | 'all',
  ): void {
    const defaults = defaultKcpDraft();
    const canonicalDraft: KcpDraft = {
      kcpNoDelay: String(settings.manualKcp.noDelay ?? defaults.kcpNoDelay),
      kcpInterval: String(settings.manualKcp.interval ?? defaults.kcpInterval),
      kcpResend: String(settings.manualKcp.resend ?? defaults.kcpResend),
      kcpNoCongestion: String(
        settings.manualKcp.noCongestion ?? defaults.kcpNoCongestion,
      ),
      kcpMtu: String(settings.kcpMtu ?? defaults.kcpMtu),
      kcpReceiveWindow: String(
        settings.kcpReceiveWindow ?? defaults.kcpReceiveWindow,
      ),
      kcpSendWindow: String(settings.kcpSendWindow ?? defaults.kcpSendWindow),
      smuxBuffer: String(settings.smuxBuffer ?? defaults.smuxBuffer),
      streamBuffer: String(settings.streamBuffer ?? defaults.streamBuffer),
      smuxKeepalive: String(settings.smuxKeepalive ?? defaults.smuxKeepalive),
      smuxTimeout: String(settings.smuxTimeout ?? defaults.smuxTimeout),
    };
    if (field === 'all') {
      kcpDraft = canonicalDraft;
      kcpErrors = {};
      kcpDraftVersions = initialKcpDraftVersions();
      return;
    }
    if (field === 'kcpMode') {
      for (const manualField of manualKcpTextFields) {
        kcpDraft[manualField] = canonicalDraft[manualField];
        delete kcpErrors[manualField];
      }
    } else if (isKcpTextField(field)) {
      kcpDraft[field] = canonicalDraft[field];
      delete kcpErrors[field];
    }
  }

  function beginCreate(): void {
    if (!settingsEditable || mutationBusy) return;
    if (draftChanged) {
      openDialog({ kind: 'discardCreate' });
      return;
    }
    startCreate();
  }

  function startCreate(): void {
    editorMode = 'create';
    draft = emptyProfileInput();
    fieldErrors = {};
    message = '';
    revealKey = false;
    void focusField('name');
  }

  function beginEdit(): void {
    if (!selectedProfile || !settingsEditable || mutationBusy) return;
    editorMode = 'edit';
    draft = profileInput(selectedProfile);
    fieldErrors = {};
    message = '';
    revealKey = false;
    void focusField('name');
  }

  function cancelEdit(): void {
    editorMode = 'view';
    draft = selectedProfile
      ? profileInput(selectedProfile)
      : emptyProfileInput();
    fieldErrors = {};
    message = '';
    revealKey = false;
  }

  async function handleProfileSelection(event: Event): Promise<void> {
    const select = event.currentTarget as HTMLSelectElement;
    const profileId = select.value;
    const currentId = selectedProfile?.id ?? '';

    if (!profileId || profileId === currentId) return;
    select.value = currentId;

    if (draftChanged) {
      openDialog({ kind: 'discardSelection', profileId });
      return;
    }
    await selectProfile(profileId);
  }

  async function selectProfile(profileId: ProfileId): Promise<void> {
    saving = true;
    message = '';
    try {
      applySnapshot(await api.selectProfile(profileId), true);
    } catch {
      message = 'The selected profile could not be opened.';
    } finally {
      saving = false;
      resolveMutationIdle();
    }
  }

  function handleFieldBlur(field: ProfileFieldName): void {
    const error = validateField(field, draft);
    if (error) {
      fieldErrors[field] = error;
    } else {
      delete fieldErrors[field];
    }
  }

  async function saveProfile(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!editorOpen || !settingsEditable || mutationBusy) return;

    fieldErrors = validateProfile(draft);
    const firstInvalid = firstInvalidField(fieldErrors);
    if (firstInvalid) {
      await focusField(firstInvalid);
      return;
    }

    const profileDraft: ProfileDraft = {
      name: draft.name,
      serverHost: draft.serverHost,
      port: Number(draft.port),
      encryptionKey: draft.encryptionKey,
    };

    saving = true;
    message = '';
    try {
      const nextSnapshot =
        editorMode === 'create'
          ? await api.createProfile(profileDraft)
          : await api.updateProfile(selectedProfile!.id, profileDraft);
      applySnapshot(nextSnapshot, true);
    } catch (error) {
      saving = false;
      resolveMutationIdle();
      await presentProfileError(error);
      return;
    }
    saving = false;
    resolveMutationIdle();
  }

  async function presentProfileError(error: unknown): Promise<void> {
    if (isIpcError(error) && error.kind === 'profileValidation') {
      fieldErrors[error.field] = validationMessage(error.field, error.issue);
      await focusField(error.field);
      return;
    }
    if (isIpcError(error) && error.kind === 'profileDuplicateName') {
      fieldErrors.name = 'A profile with this name already exists.';
      await focusField('name');
      return;
    }
    if (isIpcError(error) && error.kind === 'settingsLocked') {
      message = 'Profile settings are locked while paqet is active.';
      return;
    }
    message =
      'The profile could not be saved. Your entries have been preserved.';
  }

  function requestDelete(): void {
    if (!selectedProfile || !settingsEditable || mutationBusy) return;
    openDialog({ kind: 'delete', profile: selectedProfile });
  }

  async function confirmDialog(): Promise<void> {
    const action = dialog;
    const returnFocus = dialogInvoker;
    if (action?.kind === 'windowClose') {
      if (closeDecisionBusy) return;
      const requestId = action.request.requestId;
      closeDecisionBusy = true;
      closeDecisionMessage = '';
      try {
        await api.confirmWindowClose(requestId);
      } catch (error) {
        if (
          dialog?.kind !== 'windowClose' ||
          dialog.request.requestId !== requestId
        ) {
          return;
        }
        dismissWindowCloseDialog();
        connectionMessage =
          isIpcError(error) && error.kind === 'commandConflict'
            ? 'The close request changed before it could be confirmed.'
            : 'paqet could not finish shutting down. Use the window close control to continue supervised shutdown.';
      }
      return;
    }
    dialog = null;
    dialogInvoker = null;
    if (!action) return;

    if (action.kind === 'unsafeBlock') {
      const replacement = queueSettingsMutation(async () => {
        if (!settingsEditable) return;
        await replaceKcpSettings(
          'kcpBlock',
          (settings) => ({ ...settings, kcpBlock: action.value }),
          'kcpBlock',
        );
      });
      await tick();
      returnFocus?.focus();
      await replacement;
      return;
    }

    await waitForMutationIdle();
    if (!settingsEditable) {
      await tick();
      returnFocus?.focus();
      return;
    }

    if (action.kind === 'discardSelection') {
      cancelEdit();
      await selectProfile(action.profileId);
      await tick();
      profileSelect?.focus();
      return;
    }
    if (action.kind === 'discardCreate') {
      cancelEdit();
      startCreate();
      return;
    }

    saving = true;
    message = '';
    try {
      applySnapshot(await api.deleteProfile(action.profile.id), true);
    } catch {
      message = `The profile “${action.profile.name}” could not be deleted.`;
    } finally {
      saving = false;
      resolveMutationIdle();
    }
    await tick();
    (snapshot?.selectedProfile ? profileSelect : newProfileButton)?.focus();
  }

  function openDialog(nextDialog: Exclude<DialogState, null>): void {
    dialogInvoker =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    dialog = nextDialog;
    void tick().then(() =>
      nextDialog.kind === 'unsafeBlock' || nextDialog.kind === 'windowClose'
        ? dialogCancelButton?.focus()
        : dialogPrimaryButton?.focus(),
    );
  }

  function closeDialog(): void {
    if (dialog?.kind === 'windowClose') return;
    const returnFocus = dialogInvoker;
    dialog = null;
    dialogInvoker = null;
    void tick().then(() => returnFocus?.focus());
  }

  function handleDialogKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      if (dialog?.kind === 'windowClose') {
        void cancelWindowClose();
      } else {
        closeDialog();
      }
      return;
    }
    if (event.key !== 'Tab' || !dialogElement) return;

    const controls = Array.from(
      dialogElement.querySelectorAll<HTMLElement>('button:not(:disabled)'),
    );
    const first = controls[0];
    const last = controls.at(-1);
    if (!first || !last) return;

    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function validateProfile(input: ProfileInput): FieldErrors {
    const errors: FieldErrors = {};
    for (const field of profileFields) {
      const error = validateField(field, input);
      if (error) errors[field] = error;
    }
    return errors;
  }

  const profileFields: ProfileFieldName[] = [
    'name',
    'serverHost',
    'port',
    'encryptionKey',
  ];

  function validateField(
    field: ProfileFieldName,
    input: ProfileInput,
  ): string | undefined {
    if (field === 'name') {
      const name = input.name.trim();
      if (!name) return 'Profile name is required.';
      if (hasControlCharacter(name)) {
        return 'Profile name cannot contain control characters.';
      }
    }

    if (field === 'serverHost') {
      const host = input.serverHost.trim();
      if (!host) return 'Server IP or host is required.';
      if (!isValidIpv4(host) && !isValidHostname(host)) {
        return 'Enter a valid IPv4 address or host name.';
      }
    }

    if (field === 'port') {
      if (!input.port) return 'Port is required.';
      if (!/^\d+$/.test(input.port)) return 'Port must be a whole number.';
      const port = Number(input.port);
      if (port < 1 || port > 65_535) {
        return 'Port must be between 1 and 65535.';
      }
    }

    if (field === 'encryptionKey' && !input.encryptionKey) {
      return 'Encryption key is required.';
    }
    return undefined;
  }

  function hasControlCharacter(value: string): boolean {
    return Array.from(value).some((character) => {
      const codePoint = character.codePointAt(0) ?? 0;
      return codePoint <= 31 || (codePoint >= 127 && codePoint <= 159);
    });
  }

  function isValidIpv4(value: string): boolean {
    const parts = value.split('.');
    return (
      parts.length === 4 &&
      parts.every(
        (part) =>
          /^\d{1,3}$/.test(part) &&
          Number(part) <= 255 &&
          (part === '0' || !part.startsWith('0')),
      )
    );
  }

  function isValidHostname(value: string): boolean {
    const hostname = value.endsWith('.') ? value.slice(0, -1) : value;
    if (
      !hostname ||
      hostname.length > 253 ||
      Array.from(hostname).some((character) => character.charCodeAt(0) > 127)
    ) {
      return false;
    }
    return hostname.split('.').every((label) => {
      return (
        label.length >= 1 &&
        label.length <= 63 &&
        /^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?$/.test(label)
      );
    });
  }

  function firstInvalidField(errors: FieldErrors): ProfileFieldName | null {
    return profileFields.find((field) => errors[field]) ?? null;
  }

  async function focusField(field: ProfileFieldName): Promise<void> {
    await tick();
    const inputs: Record<ProfileFieldName, HTMLInputElement | undefined> = {
      name: nameInput,
      serverHost: serverHostInput,
      port: portInput,
      encryptionKey: encryptionKeyInput,
    };
    inputs[field]?.focus();
  }

  function isIpcError(error: unknown): error is IpcError {
    return (
      typeof error === 'object' &&
      error !== null &&
      'kind' in error &&
      typeof error.kind === 'string'
    );
  }

  function validationMessage(field: ProfileFieldName, issue: string): string {
    if (issue === 'required') {
      return field === 'name'
        ? 'Profile name is required.'
        : field === 'serverHost'
          ? 'Server IP or host is required.'
          : field === 'port'
            ? 'Port is required.'
            : 'Encryption key is required.';
    }
    if (field === 'serverHost' && issue === 'invalidFormat') {
      return 'Enter a valid IPv4 address or host name.';
    }
    if (field === 'port' && issue === 'outOfRange') {
      return 'Port must be between 1 and 65535.';
    }
    return 'Check this value and try again.';
  }

  function formatStatus(status: AppSnapshot['lifecycle']['status']): string {
    return status.charAt(0).toUpperCase() + status.slice(1);
  }

  function formatFailure(
    failure: NonNullable<LifecycleSnapshot['failure']>,
  ): string {
    if (failure.kind === 'launchFailed') {
      return 'paqet could not start. Review the connection output and configuration.';
    }
    if (failure.kind === 'connectionLost') {
      return 'The paqet client reported that the connection was lost.';
    }
    if (failure.kind === 'configurationRejected') {
      return 'The paqet client rejected the generated configuration.';
    }
    if (failure.kind === 'clientFailed') {
      return 'The paqet client reported a fatal error.';
    }
    return failure.code === null
      ? 'The paqet client exited unexpectedly.'
      : `The paqet client exited unexpectedly with code ${failure.code}.`;
  }

  async function runConnectionAction(): Promise<void> {
    if (connectionDisabled || connectionAction.kind === 'waiting') return;
    const action = connectionAction.kind;
    connectionBusy = true;
    connectionMessage = '';
    try {
      await scheduledDraftCommits;
      await settingsQueue;
      if (action !== connectionAction.kind) return;
      if (
        action === 'connect' &&
        (Object.keys(commonErrors).length > 0 ||
          Object.keys(kcpErrors).length > 0)
      ) {
        connectionMessage =
          'Correct the invalid Advanced setting before connecting.';
        return;
      }
      if (action === 'connect') {
        applySnapshot(await api.connect());
      } else {
        applySnapshot(await api.disconnect());
      }
    } catch (error) {
      presentConnectionError(error, action);
    } finally {
      connectionBusy = false;
    }
  }

  function presentConnectionError(
    error: unknown,
    action: 'connect' | 'disconnect',
  ): void {
    if (!isIpcError(error)) {
      connectionMessage =
        action === 'connect'
          ? 'paqet could not start. Review the connection output and try again.'
          : 'paqet could not finish disconnecting. Its process remains supervised.';
      return;
    }
    if (error.kind === 'profileNotSelected') {
      connectionMessage = 'Select a server profile before connecting.';
    } else if (error.kind === 'interfaceNotSelected') {
      connectionMessage =
        'Select a usable network interface before connecting.';
    } else if (error.kind === 'configValidation') {
      connectionMessage = `The ${formatConfigField(error.field)} setting is invalid. Review Advanced settings and try again.`;
    } else if (error.kind === 'configGeneration') {
      connectionMessage = 'The paqet configuration could not be generated.';
    } else if (error.kind === 'configStorage') {
      connectionMessage = 'The paqet configuration could not be saved.';
    } else if (error.kind === 'processLaunch') {
      connectionMessage =
        action === 'connect'
          ? 'The paqet client could not be started.'
          : 'The paqet process could not finish its supervised shutdown.';
    } else if (error.kind === 'runtimeSubscription') {
      connectionMessage = 'Live connection state is unavailable.';
    } else if (error.kind === 'commandConflict') {
      connectionMessage =
        'The connection state changed. Wait for it to settle and try again.';
    } else {
      connectionMessage = 'The connection action could not be completed.';
    }
  }

  function formatConfigField(field: string): string {
    return field.replace(/([A-Z])/g, ' $1').toLowerCase();
  }

  function handleLogScroll(): void {
    followingLogs = isLogAtBottom();
  }

  function isLogAtBottom(): boolean {
    if (!logElement) return true;
    return (
      logElement.scrollHeight -
        logElement.scrollTop -
        logElement.clientHeight <=
      logBottomTolerance
    );
  }

  async function scrollLogToLatest(): Promise<void> {
    await tick();
    if (!logElement) return;
    logElement.scrollTop = logElement.scrollHeight;
    followingLogs = true;
  }

  async function copyLogs(): Promise<void> {
    if (logEntries.length === 0) return;
    copyMessage = '';
    copyFailed = false;
    try {
      if (!navigator.clipboard?.writeText) throw new Error('unavailable');
      await navigator.clipboard.writeText(
        logEntries.map(formatLogEntry).join('\n'),
      );
      copyMessage = 'Logs copied.';
    } catch {
      copyFailed = true;
      copyMessage = 'Logs could not be copied to the clipboard.';
    }
  }

  function clearLogs(): void {
    logEntries = [];
    followingLogs = true;
    copyMessage = '';
    copyFailed = false;
  }

  function formatLogEntry(entry: LogEntry): string {
    if (entry.kind === 'gap') {
      return `[output unavailable: sequences ${entry.firstMissing}–${(
        BigInt(entry.nextAvailable) - 1n
      ).toString()}]`;
    }
    const stream = entry.record.stream === 'stderr' ? '[stderr] ' : '';
    const truncated = entry.record.truncated ? ' [record truncated]' : '';
    return `${stream}${entry.record.text}${truncated}`;
  }

  function logEntryKey(entry: LogEntry): string {
    return entry.kind === 'record'
      ? `${entry.sessionId}:record:${entry.record.sequence}`
      : `${entry.sessionId}:gap:${entry.firstMissing}:${entry.nextAvailable}`;
  }

  function handleWindowCloseRequest(request: WindowCloseRequest): void {
    if (dialog?.kind === 'windowClose') {
      if (dialog.request.requestId !== request.requestId) {
        closeDecisionBusy = false;
        closeDecisionMessage = '';
      }
      dialog = { kind: 'windowClose', request };
      return;
    }
    closeDecisionBusy = false;
    closeDecisionMessage = '';
    openDialog({ kind: 'windowClose', request });
  }

  async function cancelWindowClose(): Promise<void> {
    const action = dialog;
    if (action?.kind !== 'windowClose' || closeDecisionBusy) {
      return;
    }
    const requestId = action.request.requestId;
    closeDecisionBusy = true;
    closeDecisionMessage = '';
    try {
      await api.cancelWindowClose(requestId);
      if (
        dialog?.kind === 'windowClose' &&
        dialog.request.requestId === requestId
      ) {
        dismissWindowCloseDialog();
      }
    } catch {
      if (
        dialog?.kind !== 'windowClose' ||
        dialog.request.requestId !== requestId
      ) {
        return;
      }
      if (snapshot?.lifecycle.settingsEditable) {
        dismissWindowCloseDialog();
      } else {
        closeDecisionMessage =
          'The close request changed before it could be canceled.';
        closeDecisionBusy = false;
      }
    }
  }

  function dismissWindowCloseDialog(): void {
    const returnFocus = dialogInvoker;
    dialog = null;
    dialogInvoker = null;
    closeDecisionBusy = false;
    closeDecisionMessage = '';
    void tick().then(() => returnFocus?.focus());
  }

  async function handleInterfaceSelection(event: Event): Promise<void> {
    const select = event.currentTarget as HTMLSelectElement;
    const guid = select.value;
    select.value = snapshot?.selectedInterfaceGuid ?? '';
    if (!guid || guid === snapshot?.selectedInterfaceGuid) return;
    await runInterfaceMutation('select', () => api.selectInterface(guid));
  }

  async function refreshInterfaces(): Promise<void> {
    await runInterfaceMutation('refresh', () => api.refreshInterfaces());
  }

  async function runInterfaceMutation(
    kind: Exclude<InterfaceOperation, null>,
    operation: () => Promise<AppSnapshot>,
  ): Promise<void> {
    if (!settingsEditable || mutationBusy) return;
    interfaceOperation = kind;
    interfaceMessage = '';
    try {
      applySnapshot(await operation());
    } catch (error) {
      presentInterfaceError(error);
    } finally {
      interfaceOperation = null;
      resolveMutationIdle();
    }
  }

  function presentInterfaceError(error: unknown): void {
    if (isIpcError(error) && error.kind === 'settingsLocked') {
      interfaceMessage = 'Network settings are locked while paqet is active.';
      return;
    }
    if (isIpcError(error) && error.kind === 'interfaceNotFound') {
      interfaceMessage =
        'That network interface is no longer available. Refresh the list and choose another.';
      return;
    }
    if (isIpcError(error) && error.kind === 'networkDiscovery') {
      interfaceMessage =
        'Windows network interfaces could not be refreshed. Check your network and try again.';
      return;
    }
    interfaceMessage = 'The network interface could not be updated.';
  }

  function interfaceOptionLabel(networkInterface: NetworkInterface): string {
    return `${networkInterface.friendlyName} · ${networkInterface.localAddress}`;
  }

  async function toggleCommonOverride(
    field: CommonSettingField,
    enabled: boolean,
  ): Promise<void> {
    await queueSettingsMutation(async () => {
      if (!snapshot || !settingsEditable) return;
      if (!enabled) {
        await replaceCommonSetting(field, null);
        return;
      }

      if (field === 'logLevel') {
        await replaceCommonSetting(field, 'info');
        return;
      }

      const parsed = parseCommonDraft(field, commonDraft[field]);
      if (typeof parsed === 'string') {
        commonErrors[field] = parsed;
        await focusCommonField(field);
        return;
      }
      delete commonErrors[field];
      await replaceCommonSetting(field, parsed.value);
    });
  }

  async function selectLogLevel(event: Event): Promise<void> {
    const select = event.currentTarget as HTMLSelectElement;
    const value = select.value as LogLevel;
    select.value = snapshot?.advancedSettings.logLevel ?? 'info';
    await queueSettingsMutation(() => replaceCommonSetting('logLevel', value));
  }

  function scheduleCommonDraftCommit(field: CommonTextField): void {
    scheduledDraftCommits = scheduledDraftCommits.then(
      () =>
        new Promise<void>((resolve) => {
          window.setTimeout(() => {
            void commitCommonDraft(field).finally(resolve);
          }, 0);
        }),
    );
  }

  async function commitCommonDraft(field: CommonTextField): Promise<void> {
    const input = commonDraft[field];
    const draftVersion = commonDraftVersions[field];
    await queueSettingsMutation(async () => {
      if (
        !snapshot ||
        !settingsEditable ||
        snapshot.advancedSettings[field] === null
      ) {
        return;
      }

      const parsed = parseCommonDraft(field, input);
      if (typeof parsed === 'string') {
        commonErrors[field] = parsed;
        return;
      }
      delete commonErrors[field];
      if (
        commonSettingMatches(snapshot.advancedSettings[field], parsed.value)
      ) {
        if (commonDraftVersions[field] === draftVersion) {
          commonDraft[field] = parsed.normalized;
        }
        return;
      }
      await replaceCommonSetting(field, parsed.value, draftVersion);
    });
  }

  function handleCommonInput(field: CommonTextField): void {
    commonDraftVersions[field] += 1;
    delete commonErrors[field];
    settingsMessage = '';
  }

  function handleCommonKeydown(event: KeyboardEvent): void {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    (event.currentTarget as HTMLInputElement).blur();
  }

  async function replaceCommonSetting(
    field: CommonSettingField,
    value: AdvancedSettings[CommonSettingField],
    draftVersion?: number,
  ): Promise<void> {
    if (
      !snapshot ||
      !settingsEditable ||
      saving ||
      interfaceBusy ||
      settingsBusy
    ) {
      return;
    }
    settingsOperation = field;
    settingsMessage = '';
    try {
      applySnapshot(
        await api.replaceAdvancedSettings({
          ...snapshot.advancedSettings,
          [field]: value,
        }),
        false,
        draftVersion === undefined ||
          (field !== 'logLevel' && commonDraftVersions[field] === draftVersion)
          ? field
          : null,
      );
    } catch (error) {
      await presentSettingsError(error, field);
    } finally {
      settingsOperation = null;
      resolveMutationIdle();
    }
  }

  async function presentSettingsError(
    error: unknown,
    attemptedField: CommonSettingField,
  ): Promise<void> {
    if (
      isIpcError(error) &&
      error.kind === 'configValidation' &&
      isCommonTextField(error.field)
    ) {
      commonErrors[error.field] = commonValidationMessage(error.field);
      await focusCommonField(error.field);
      return;
    }
    settingsMessage =
      isIpcError(error) && error.kind === 'settingsLocked'
        ? 'Advanced settings are locked while paqet is active.'
        : `The ${commonSettingLabel(attemptedField)} override could not be updated.`;
  }

  function parseCommonDraft(
    field: CommonTextField,
    input: string,
  ): string | { value: number | string | string[]; normalized: string } {
    const value = input.trim();
    if (field === 'localTcpFlags' || field === 'remoteTcpFlags') {
      const combinations = value
        .split(',')
        .map((combination) => combination.trim());
      if (
        combinations.length === 0 ||
        combinations.length > 64 ||
        combinations.some((combination) => !/^[FSRPAUECN]+$/.test(combination))
      ) {
        return 'Enter 1–64 comma-separated uppercase combinations using F S R P A U E C N.';
      }
      return { value: combinations, normalized: combinations.join(', ') };
    }

    if (!/^\d+$/.test(value)) {
      return 'Enter a whole number using decimal digits.';
    }
    const parsed = BigInt(value);
    const [minimum, maximum] = commonRange(field);
    if (parsed < minimum || parsed > maximum) {
      return commonValidationMessage(field);
    }
    return {
      value:
        field === 'pcapSocketBuffer' || field === 'connectionCount'
          ? Number(parsed)
          : parsed.toString(),
      normalized: parsed.toString(),
    };
  }

  function commonRange(
    field: Exclude<CommonTextField, 'localTcpFlags' | 'remoteTcpFlags'>,
  ): [bigint, bigint] {
    if (field === 'pcapSocketBuffer') return [1024n, 104857600n];
    if (field === 'connectionCount') return [1n, 256n];
    if (field === 'tcpBuffer') return [4096n, 9223372036854775807n];
    return [2048n, 9223372036854775807n];
  }

  function commonValidationMessage(field: CommonTextField): string {
    if (field === 'pcapSocketBuffer') {
      return 'PCAP socket buffer must be between 1024 and 104857600 bytes.';
    }
    if (field === 'connectionCount') {
      return 'Connection count must be between 1 and 256.';
    }
    if (field === 'tcpBuffer') {
      return 'TCP buffer must be between 4096 and 9223372036854775807 bytes.';
    }
    if (field === 'udpBuffer') {
      return 'UDP buffer must be between 2048 and 9223372036854775807 bytes.';
    }
    return 'Enter 1–64 comma-separated uppercase combinations using F S R P A U E C N.';
  }

  function commonSettingMatches(
    current: AdvancedSettings[CommonTextField],
    next: number | string | string[],
  ): boolean {
    return Array.isArray(current) && Array.isArray(next)
      ? current.length === next.length &&
          current.every((value, index) => value === next[index])
      : current === next;
  }

  function isCommonTextField(field: string): field is CommonTextField {
    return commonTextFields.includes(field as CommonTextField);
  }

  function isCommonSettingField(field: string): field is CommonSettingField {
    return field === 'logLevel' || isCommonTextField(field);
  }

  const commonTextFields: CommonTextField[] = [
    'pcapSocketBuffer',
    'localTcpFlags',
    'remoteTcpFlags',
    'connectionCount',
    'tcpBuffer',
    'udpBuffer',
  ];

  const flagFields = ['localTcpFlags', 'remoteTcpFlags'] as const;
  const numericFields = [
    {
      field: 'connectionCount' as const,
      title: 'connection count',
      label: 'Connection count',
      defaultValue: '1',
      hint: '1–256 connections',
    },
    {
      field: 'tcpBuffer' as const,
      title: 'TCP buffer',
      label: 'TCP buffer',
      defaultValue: '8192 bytes',
      hint: '4096–9223372036854775807 bytes',
    },
    {
      field: 'udpBuffer' as const,
      title: 'UDP buffer',
      label: 'UDP buffer',
      defaultValue: '4096 bytes',
      hint: '2048–9223372036854775807 bytes',
    },
  ];
  const manualKcpTextFields: KcpTextField[] = [
    'kcpNoDelay',
    'kcpInterval',
    'kcpResend',
    'kcpNoCongestion',
  ];
  const kcpTextFields: KcpTextField[] = [
    ...manualKcpTextFields,
    'kcpMtu',
    'kcpReceiveWindow',
    'kcpSendWindow',
    'smuxBuffer',
    'streamBuffer',
    'smuxKeepalive',
    'smuxTimeout',
  ];
  const kcpSettingFields: KcpSettingField[] = [
    'kcpMode',
    ...kcpTextFields,
    'kcpWriteDelay',
    'kcpAckNoDelay',
    'kcpBlock',
  ];
  const kcpNumericFields = [
    {
      field: 'kcpMtu' as const,
      title: 'KCP MTU',
      label: 'KCP MTU',
      defaultValue: '1350 bytes',
      hint: '50–1500 bytes',
    },
    {
      field: 'kcpReceiveWindow' as const,
      title: 'KCP receive window',
      label: 'KCP receive window',
      defaultValue: '512',
      hint: '1–32768',
    },
    {
      field: 'kcpSendWindow' as const,
      title: 'KCP send window',
      label: 'KCP send window',
      defaultValue: '512',
      hint: '1–32768',
    },
    {
      field: 'smuxBuffer' as const,
      title: 'SMUX buffer',
      label: 'SMUX buffer',
      defaultValue: '4194304 bytes',
      hint: '1024–2147483647 bytes',
    },
    {
      field: 'streamBuffer' as const,
      title: 'stream buffer',
      label: 'Stream buffer',
      defaultValue: '2097152 bytes',
      hint: '1024–2147483647 bytes; no larger than SMUX buffer',
    },
    {
      field: 'smuxKeepalive' as const,
      title: 'SMUX keepalive',
      label: 'SMUX keepalive',
      defaultValue: '2 seconds',
      hint: '1–4294967295 seconds; no longer than timeout',
    },
    {
      field: 'smuxTimeout' as const,
      title: 'SMUX timeout',
      label: 'SMUX timeout',
      defaultValue: '8 seconds',
      hint: '1–4294967295 seconds; at least keepalive',
    },
  ];
  const manualKcpFields = [
    {
      field: 'kcpNoDelay' as const,
      label: 'KCP nodelay',
      hint: '0 or 1',
    },
    {
      field: 'kcpInterval' as const,
      label: 'KCP interval',
      hint: '10–5000 milliseconds',
    },
    {
      field: 'kcpResend' as const,
      label: 'KCP resend',
      hint: '0–2',
    },
    {
      field: 'kcpNoCongestion' as const,
      label: 'KCP nocongestion',
      hint: '0 or 1',
    },
  ];

  function commonSettingLabel(field: CommonSettingField): string {
    const labels: Record<CommonSettingField, string> = {
      logLevel: 'log level',
      pcapSocketBuffer: 'PCAP socket buffer',
      localTcpFlags: 'local TCP flags',
      remoteTcpFlags: 'remote TCP flags',
      connectionCount: 'connection count',
      tcpBuffer: 'TCP buffer',
      udpBuffer: 'UDP buffer',
    };
    return labels[field];
  }

  async function focusCommonField(field: CommonTextField): Promise<void> {
    await tick();
    commonFieldInputs[field]?.focus();
  }

  async function toggleKcpOverride(
    field: IndependentKcpField,
    enabled: boolean,
  ): Promise<void> {
    if (field === 'kcpMode') kcpModeQueued = true;
    try {
      await queueSettingsMutation(async () => {
        if (!snapshot || !settingsEditable) return;
        if (!enabled) {
          const candidate =
            field === 'kcpMode'
              ? {
                  ...snapshot.advancedSettings,
                  kcpMode: null,
                  manualKcp: emptyManualKcpSettings(),
                }
              : { ...snapshot.advancedSettings, [field]: null };
          if (
            field === 'smuxBuffer' ||
            field === 'streamBuffer' ||
            field === 'smuxKeepalive' ||
            field === 'smuxTimeout'
          ) {
            const relationshipError = validateKcpRelationships(
              candidate,
              field,
            );
            if (relationshipError) {
              transportMessage = relationshipError;
              return;
            }
          }
          await replaceKcpSettings(field, () => candidate, field);
          return;
        }
        if (field === 'kcpMode') {
          await replaceKcpSettings(
            field,
            (settings) => ({ ...settings, kcpMode: 'fast' }),
            field,
          );
          return;
        }
        if (field === 'kcpBlock') {
          await replaceKcpSettings(
            field,
            (settings) => ({ ...settings, kcpBlock: 'aes' }),
            field,
          );
          return;
        }

        const parsed = parseKcpDraft(field, kcpDraft[field]);
        if (typeof parsed === 'string') {
          kcpErrors[field] = parsed;
          await focusKcpField(field);
          return;
        }
        const candidate = {
          ...snapshot.advancedSettings,
          [field]: parsed.value,
        };
        const relationshipError = validateKcpRelationships(candidate, field);
        if (relationshipError) {
          kcpErrors[field] = relationshipError;
          await focusKcpField(field);
          return;
        }
        delete kcpErrors[field];
        await replaceKcpSettings(field, () => candidate, field);
      });
    } finally {
      if (field === 'kcpMode') kcpModeQueued = false;
    }
  }

  async function handleKcpOverrideToggle(
    event: Event,
    field: IndependentKcpField,
  ): Promise<void> {
    const checkbox = event.currentTarget as HTMLInputElement;
    const enabled = checkbox.checked;
    checkbox.checked = snapshot?.advancedSettings[field] !== null;
    await toggleKcpOverride(field, enabled);
  }

  async function selectKcpMode(event: Event): Promise<void> {
    const select = event.currentTarget as HTMLSelectElement;
    const value = select.value as KcpMode;
    select.value = snapshot?.advancedSettings.kcpMode ?? 'fast';
    kcpModeQueued = true;
    try {
      await queueSettingsMutation(() =>
        replaceKcpSettings(
          'kcpMode',
          (settings) => {
            const currentMode = settings.kcpMode ?? 'fast';
            return {
              ...settings,
              kcpMode: value,
              manualKcp:
                value === 'manual'
                  ? manualKcpPreset(currentMode, settings.manualKcp)
                  : emptyManualKcpSettings(),
            };
          },
          'kcpMode',
        ),
      );
    } finally {
      kcpModeQueued = false;
    }
  }

  async function toggleManualKcpBoolean(
    field: 'kcpWriteDelay' | 'kcpAckNoDelay',
    value: boolean,
  ): Promise<void> {
    const manualField = field === 'kcpWriteDelay' ? 'writeDelay' : 'ackNoDelay';
    await queueSettingsMutation(async () => {
      if (snapshot?.advancedSettings.kcpMode !== 'manual') return;
      await replaceKcpSettings(
        field,
        (settings) => ({
          ...settings,
          manualKcp: { ...settings.manualKcp, [manualField]: value },
        }),
        field,
      );
    });
  }

  async function handleManualKcpBoolean(
    event: Event,
    field: 'kcpWriteDelay' | 'kcpAckNoDelay',
  ): Promise<void> {
    const checkbox = event.currentTarget as HTMLInputElement;
    const value = checkbox.checked;
    const manualField = field === 'kcpWriteDelay' ? 'writeDelay' : 'ackNoDelay';
    checkbox.checked =
      snapshot?.advancedSettings.manualKcp[manualField] ?? false;
    await toggleManualKcpBoolean(field, value);
  }

  async function selectKcpBlock(event: Event): Promise<void> {
    const select = event.currentTarget as HTMLSelectElement;
    const value = select.value as KcpBlock;
    select.value = snapshot?.advancedSettings.kcpBlock ?? 'aes';
    if (value === 'none' || value === 'null') {
      openDialog({ kind: 'unsafeBlock', value });
      return;
    }
    await queueSettingsMutation(() =>
      replaceKcpSettings(
        'kcpBlock',
        (settings) => ({ ...settings, kcpBlock: value }),
        'kcpBlock',
      ),
    );
  }

  function scheduleKcpDraftCommit(field: KcpTextField): void {
    scheduledDraftCommits = scheduledDraftCommits.then(
      () =>
        new Promise<void>((resolve) => {
          window.setTimeout(() => {
            void commitKcpDraft(field).finally(resolve);
          }, 0);
        }),
    );
  }

  async function commitKcpDraft(field: KcpTextField): Promise<void> {
    const input = kcpDraft[field];
    const draftVersion = kcpDraftVersions[field];
    await queueSettingsMutation(async () => {
      if (!snapshot || !settingsEditable || !kcpFieldEnabled(field)) return;
      const parsed = parseKcpDraft(field, input);
      if (typeof parsed === 'string') {
        kcpErrors[field] = parsed;
        return;
      }
      const candidate = updateKcpTextSetting(
        snapshot.advancedSettings,
        field,
        parsed.value,
      );
      const relationshipError = validateKcpRelationships(candidate, field);
      if (relationshipError) {
        kcpErrors[field] = relationshipError;
        return;
      }
      delete kcpErrors[field];
      if (
        kcpTextSettingValue(snapshot.advancedSettings, field) === parsed.value
      ) {
        if (kcpDraftVersions[field] === draftVersion) {
          kcpDraft[field] = parsed.normalized;
        }
        return;
      }
      await replaceKcpSettings(field, () => candidate, field, draftVersion);
    });
  }

  function handleKcpInput(field: KcpTextField): void {
    kcpDraftVersions[field] += 1;
    delete kcpErrors[field];
    transportMessage = '';
  }

  async function replaceKcpSettings(
    attemptedField: KcpSettingField,
    update: (settings: AdvancedSettings) => AdvancedSettings,
    fieldToSync: KcpSettingField,
    draftVersion?: number,
  ): Promise<void> {
    if (
      !snapshot ||
      !settingsEditable ||
      saving ||
      interfaceBusy ||
      settingsBusy
    ) {
      return;
    }
    settingsOperation = attemptedField;
    transportMessage = '';
    try {
      applySnapshot(
        await api.replaceAdvancedSettings(update(snapshot.advancedSettings)),
        false,
        draftVersion === undefined ||
          !isKcpTextField(fieldToSync) ||
          kcpDraftVersions[fieldToSync] === draftVersion
          ? fieldToSync
          : null,
      );
    } catch (error) {
      await presentKcpSettingsError(error, attemptedField);
    } finally {
      settingsOperation = null;
      resolveMutationIdle();
    }
  }

  async function presentKcpSettingsError(
    error: unknown,
    attemptedField: KcpSettingField,
  ): Promise<void> {
    transportMessage =
      isIpcError(error) && error.kind === 'settingsLocked'
        ? 'Advanced settings are locked while paqet is active.'
        : `The ${settingsFieldLabel(attemptedField)} override could not be updated.`;
  }

  function parseKcpDraft(
    field: KcpTextField,
    input: string,
  ): string | { value: number; normalized: string } {
    const value = input.trim();
    if (!/^\d+$/.test(value)) {
      return 'Enter a whole number using decimal digits.';
    }
    const parsed = BigInt(value);
    const [minimum, maximum] = kcpRange(field);
    if (parsed < minimum || parsed > maximum) {
      return kcpValidationMessage(field);
    }
    return { value: Number(parsed), normalized: parsed.toString() };
  }

  function kcpRange(field: KcpTextField): [bigint, bigint] {
    if (field === 'kcpNoDelay' || field === 'kcpNoCongestion') return [0n, 1n];
    if (field === 'kcpInterval') return [10n, 5000n];
    if (field === 'kcpResend') return [0n, 2n];
    if (field === 'kcpMtu') return [50n, 1500n];
    if (field === 'kcpReceiveWindow' || field === 'kcpSendWindow') {
      return [1n, 32768n];
    }
    if (field === 'smuxBuffer' || field === 'streamBuffer') {
      return [1024n, 2147483647n];
    }
    return [1n, 4294967295n];
  }

  function kcpValidationMessage(field: KcpTextField): string {
    const [minimum, maximum] = kcpRange(field);
    return `${settingsFieldLabel(field)} must be between ${minimum} and ${maximum}.`;
  }

  function validateKcpRelationships(
    settings: AdvancedSettings,
    attemptedField: KcpTextField,
  ): string | undefined {
    const smuxBuffer = settings.smuxBuffer ?? 4_194_304;
    const streamBuffer = settings.streamBuffer ?? 2_097_152;
    if (streamBuffer > smuxBuffer) {
      return attemptedField === 'smuxBuffer'
        ? 'SMUX buffer must be at least the effective stream buffer.'
        : 'Stream buffer must not exceed the effective SMUX buffer.';
    }
    const keepalive = settings.smuxKeepalive ?? 2;
    const timeout = settings.smuxTimeout ?? 8;
    if (timeout < keepalive) {
      return attemptedField === 'smuxKeepalive'
        ? 'SMUX keepalive must not exceed the effective timeout.'
        : 'SMUX timeout must be at least the effective keepalive.';
    }
    return undefined;
  }

  function updateKcpTextSetting(
    settings: AdvancedSettings,
    field: KcpTextField,
    value: number,
  ): AdvancedSettings {
    const manualField = manualKcpProperty(field);
    return manualField
      ? {
          ...settings,
          manualKcp: { ...settings.manualKcp, [manualField]: value },
        }
      : { ...settings, [field]: value };
  }

  function kcpTextSettingValue(
    settings: AdvancedSettings,
    field: KcpTextField,
  ): number | null {
    const manualField = manualKcpProperty(field);
    return manualField
      ? settings.manualKcp[manualField]
      : settings[field as IndependentKcpTextField];
  }

  function manualKcpProperty(
    field: KcpTextField,
  ):
    | keyof Pick<
        ManualKcpSettings,
        'noDelay' | 'interval' | 'resend' | 'noCongestion'
      >
    | null {
    const properties = {
      kcpNoDelay: 'noDelay',
      kcpInterval: 'interval',
      kcpResend: 'resend',
      kcpNoCongestion: 'noCongestion',
    } as const;
    return field in properties
      ? properties[field as keyof typeof properties]
      : null;
  }

  function kcpFieldEnabled(field: KcpTextField): boolean {
    if (!snapshot) return false;
    const manualField = manualKcpProperty(field);
    return manualField
      ? snapshot.advancedSettings.kcpMode === 'manual'
      : snapshot.advancedSettings[field as IndependentKcpTextField] !== null;
  }

  function manualKcpPreset(
    mode: KcpMode,
    currentManual: ManualKcpSettings,
  ): ManualKcpSettings {
    const presets: Record<Exclude<KcpMode, 'manual'>, ManualKcpSettings> = {
      normal: manualKcp(0, 40, 2, 1, true, false),
      fast: manualKcp(0, 30, 2, 1, true, false),
      fast2: manualKcp(1, 20, 2, 1, false, true),
      fast3: manualKcp(1, 10, 2, 1, false, true),
    };
    return mode === 'manual' ? { ...currentManual } : presets[mode];
  }

  function manualKcp(
    noDelay: number,
    interval: number,
    resend: number,
    noCongestion: number,
    writeDelay: boolean,
    ackNoDelay: boolean,
  ): ManualKcpSettings {
    return { noDelay, interval, resend, noCongestion, writeDelay, ackNoDelay };
  }

  function emptyManualKcpSettings(): ManualKcpSettings {
    return {
      noDelay: null,
      interval: null,
      resend: null,
      noCongestion: null,
      writeDelay: null,
      ackNoDelay: null,
    };
  }

  function isKcpTextField(field: string): field is KcpTextField {
    return kcpTextFields.includes(field as KcpTextField);
  }

  function isKcpSettingField(field: string): field is KcpSettingField {
    return kcpSettingFields.includes(field as KcpSettingField);
  }

  function settingsFieldLabel(field: SettingsField): string {
    if (isCommonSettingField(field)) return commonSettingLabel(field);
    const item = kcpNumericFields.find(
      (candidate) => candidate.field === field,
    );
    if (item) return item.label;
    const labels: Partial<Record<KcpSettingField, string>> = {
      kcpMode: 'KCP mode',
      kcpNoDelay: 'KCP nodelay',
      kcpInterval: 'KCP interval',
      kcpResend: 'KCP resend',
      kcpNoCongestion: 'KCP nocongestion',
      kcpWriteDelay: 'KCP write delay',
      kcpAckNoDelay: 'KCP ACK nodelay',
      kcpBlock: 'KCP block',
    };
    return labels[field] ?? field;
  }

  async function focusKcpField(field: KcpTextField): Promise<void> {
    await tick();
    kcpFieldInputs[field]?.focus();
  }

  function waitForMutationIdle(): Promise<void> {
    return mutationBusy
      ? new Promise((resolve) => mutationIdleResolvers.push(resolve))
      : Promise.resolve();
  }

  function queueSettingsMutation(
    operation: () => Promise<void>,
  ): Promise<void> {
    const queued = settingsQueue.then(async () => {
      await waitForMutationIdle();
      await operation();
    });
    settingsQueue = queued.catch(() => undefined);
    return queued;
  }

  function resolveMutationIdle(): void {
    if (saving || interfaceOperation !== null || settingsOperation !== null) {
      return;
    }
    const resolvers = mutationIdleResolvers;
    mutationIdleResolvers = [];
    for (const resolve of resolvers) resolve();
  }
</script>

<svelte:head>
  <meta
    name="description"
    content="PaqetGUI is a lightweight Windows desktop client for paqet."
  />
</svelte:head>

<main
  class="app-shell"
  inert={dialog ? true : undefined}
  aria-hidden={dialog ? 'true' : undefined}
>
  <header class="topbar">
    <div class="wordmark">
      <span class="wordmark-mark" aria-hidden="true"></span>
      <h1>PaqetGUI</h1>
    </div>
    <p
      class:status-failed={statusLabel === 'Failed'}
      class:status-connected={statusLabel === 'Connected'}
      class:status-pending={statusLabel === 'Connecting' ||
        statusLabel === 'Disconnecting'}
      class="status"
      aria-label="Connection status"
      aria-live="polite"
    >
      <span aria-hidden="true"></span>
      {statusLabel}
    </p>
  </header>

  <section
    class="configuration"
    aria-labelledby="profile-heading"
    aria-busy={connectionBusy}
    inert={connectionBusy ? true : undefined}
  >
    <div class="section-heading">
      <div>
        <p class="eyebrow">Configuration</p>
        <h2 id="profile-heading">Server profile</h2>
      </div>
      <span class="section-count">
        {snapshot?.profiles.length ?? 0}
        {(snapshot?.profiles.length ?? 0) === 1 ? 'profile' : 'profiles'}
      </span>
    </div>

    {#if loading}
      <div class="empty-state" role="status">Loading profiles…</div>
    {:else if !snapshot}
      <div class="empty-state">
        <p>Profiles are unavailable.</p>
        <button
          class="secondary-button compact-button"
          type="button"
          onclick={loadSnapshot}
        >
          Try again
        </button>
      </div>
    {:else}
      <div class="profile-toolbar">
        <label class="sr-only" for="profile-select"
          >Selected server profile</label
        >
        <select
          id="profile-select"
          bind:this={profileSelect}
          value={selectedProfile?.id ?? ''}
          disabled={!settingsEditable ||
            mutationBusy ||
            snapshot.profiles.length === 0}
          onchange={handleProfileSelection}
        >
          {#if snapshot.profiles.length === 0}
            <option value="">No profiles saved</option>
          {/if}
          {#each snapshot.profiles as profile (profile.id)}
            <option value={profile.id}>{profile.name}</option>
          {/each}
        </select>
        <button
          class="secondary-button compact-button"
          type="button"
          bind:this={newProfileButton}
          disabled={!settingsEditable || mutationBusy}
          onclick={beginCreate}
        >
          New
        </button>
        <button
          class="secondary-button compact-button"
          type="button"
          disabled={!selectedProfile ||
            !settingsEditable ||
            mutationBusy ||
            editorOpen}
          onclick={beginEdit}
        >
          Edit
        </button>
      </div>

      {#if !selectedProfile && editorMode === 'view'}
        <div class="empty-state">
          <p>Add a server profile to begin configuring paqet.</p>
          <button
            class="secondary-button"
            type="button"
            disabled={!settingsEditable || mutationBusy}
            onclick={beginCreate}
          >
            Add server profile
          </button>
        </div>
      {:else}
        <form
          class="profile-form"
          aria-label="Server profile"
          onsubmit={saveProfile}
          novalidate
        >
          <div class="field field-name">
            <label for="profile-name">Profile name</label>
            <input
              id="profile-name"
              bind:this={nameInput}
              bind:value={draft.name}
              readonly={!editorOpen || !settingsEditable}
              disabled={saving}
              autocomplete="off"
              required={editorOpen}
              aria-invalid={fieldErrors.name ? 'true' : undefined}
              aria-describedby={fieldErrors.name
                ? 'profile-name-error'
                : undefined}
              onblur={() => editorOpen && handleFieldBlur('name')}
            />
            {#if fieldErrors.name}
              <p class="field-error" id="profile-name-error">
                {fieldErrors.name}
              </p>
            {/if}
          </div>

          <div class="server-row">
            <div class="field field-host">
              <label for="server-host">Server IP or host</label>
              <input
                id="server-host"
                bind:this={serverHostInput}
                bind:value={draft.serverHost}
                readonly={!editorOpen || !settingsEditable}
                disabled={saving}
                autocapitalize="none"
                autocomplete="off"
                spellcheck="false"
                required={editorOpen}
                aria-invalid={fieldErrors.serverHost ? 'true' : undefined}
                aria-describedby={fieldErrors.serverHost
                  ? 'server-host-error'
                  : undefined}
                onblur={() => editorOpen && handleFieldBlur('serverHost')}
              />
              {#if fieldErrors.serverHost}
                <p class="field-error" id="server-host-error">
                  {fieldErrors.serverHost}
                </p>
              {/if}
            </div>

            <div class="field field-port">
              <label for="server-port">Port</label>
              <input
                id="server-port"
                bind:this={portInput}
                bind:value={draft.port}
                readonly={!editorOpen || !settingsEditable}
                disabled={saving}
                inputmode="numeric"
                autocomplete="off"
                required={editorOpen}
                aria-invalid={fieldErrors.port ? 'true' : undefined}
                aria-describedby={fieldErrors.port
                  ? 'server-port-error'
                  : undefined}
                onblur={() => editorOpen && handleFieldBlur('port')}
              />
              {#if fieldErrors.port}
                <p class="field-error" id="server-port-error">
                  {fieldErrors.port}
                </p>
              {/if}
            </div>
          </div>

          <div class="field">
            <label for="encryption-key">Encryption key</label>
            <div class="secret-control">
              <input
                id="encryption-key"
                bind:this={encryptionKeyInput}
                bind:value={draft.encryptionKey}
                type={revealKey ? 'text' : 'password'}
                readonly={!editorOpen || !settingsEditable}
                disabled={saving}
                autocomplete="off"
                spellcheck="false"
                required={editorOpen}
                aria-invalid={fieldErrors.encryptionKey ? 'true' : undefined}
                aria-describedby={fieldErrors.encryptionKey
                  ? 'encryption-key-error'
                  : undefined}
                onblur={() => editorOpen && handleFieldBlur('encryptionKey')}
              />
              <button
                class="text-button reveal-button"
                type="button"
                aria-pressed={revealKey}
                aria-label={revealKey
                  ? 'Conceal encryption key'
                  : 'Reveal encryption key'}
                disabled={saving || !draft.encryptionKey}
                onclick={() => (revealKey = !revealKey)}
              >
                {revealKey ? 'Hide' : 'Show'}
              </button>
            </div>
            {#if fieldErrors.encryptionKey}
              <p class="field-error" id="encryption-key-error">
                {fieldErrors.encryptionKey}
              </p>
            {/if}
          </div>

          {#if editorOpen}
            <div class="form-actions">
              {#if editorMode === 'edit'}
                <button
                  class="danger-button"
                  type="button"
                  disabled={mutationBusy || !settingsEditable}
                  onclick={requestDelete}
                >
                  Delete profile
                </button>
              {/if}
              <span class="action-spacer"></span>
              <button
                class="text-button"
                type="button"
                disabled={saving}
                onclick={cancelEdit}
              >
                Cancel
              </button>
              <button
                class="primary-small"
                type="submit"
                disabled={mutationBusy || !settingsEditable}
              >
                {saving
                  ? 'Saving…'
                  : editorMode === 'create'
                    ? 'Save profile'
                    : 'Save changes'}
              </button>
            </div>
          {/if}
        </form>
      {/if}

      <div class="advanced-shell">
        <button
          class="disclosure-button"
          type="button"
          aria-expanded={advancedExpanded}
          aria-controls="advanced-content"
          onclick={() => (advancedExpanded = !advancedExpanded)}
        >
          <span>
            <strong>Advanced</strong>
            <small>Network interface and paqet overrides</small>
          </span>
          <span class="chevron" aria-hidden="true"></span>
        </button>
        {#if advancedExpanded}
          <div id="advanced-content" class="advanced-content">
            <section
              class="interface-section"
              aria-labelledby="interface-heading"
            >
              <div class="advanced-section-heading">
                <div>
                  <h3 id="interface-heading">Network interface</h3>
                  <p>Used to derive the local paqet connection details.</p>
                </div>
                <button
                  class="text-button refresh-button"
                  type="button"
                  disabled={!settingsEditable || mutationBusy}
                  onclick={refreshInterfaces}
                >
                  {interfaceOperation === 'refresh' ? 'Refreshing…' : 'Refresh'}
                </button>
              </div>

              <div class="field">
                <label for="interface-select">Interface</label>
                <select
                  id="interface-select"
                  class="interface-select"
                  value={snapshot.selectedInterfaceGuid ?? ''}
                  disabled={!settingsEditable ||
                    mutationBusy ||
                    snapshot.interfaces.length === 0}
                  onchange={handleInterfaceSelection}
                >
                  {#if snapshot.interfaces.length === 0}
                    <option value="">No usable interfaces found</option>
                  {:else if !snapshot.selectedInterfaceGuid}
                    <option value="">Select an interface</option>
                  {/if}
                  {#each snapshot.interfaces as networkInterface (networkInterface.guid)}
                    <option value={networkInterface.guid}>
                      {interfaceOptionLabel(networkInterface)}
                    </option>
                  {/each}
                </select>
              </div>

              {#if interfaceOperation === 'select'}
                <p class="interface-progress" role="status" aria-live="polite">
                  Selecting network interface…
                </p>
              {/if}

              {#if interfaceMessage}
                <p class="inline-message" role="alert">{interfaceMessage}</p>
              {/if}

              {#if selectedInterface}
                <dl
                  class="interface-details"
                  aria-label="Derived interface details"
                >
                  <div>
                    <dt>Interface name</dt>
                    <dd>{selectedInterface.interfaceName}</dd>
                  </div>
                  <div>
                    <dt>Npcap device</dt>
                    <dd>{selectedInterface.guid}</dd>
                  </div>
                  <div>
                    <dt>Local address</dt>
                    <dd>{selectedInterface.localAddress}</dd>
                  </div>
                  <div>
                    <dt>Gateway address</dt>
                    <dd>{selectedInterface.gatewayAddress}</dd>
                  </div>
                  <div>
                    <dt>Gateway MAC</dt>
                    <dd>{selectedInterface.gatewayMac}</dd>
                  </div>
                </dl>
              {:else}
                <div class="interface-empty" role="status">
                  <strong>No usable network interface is available.</strong>
                  <span>Connect Ethernet or Wi-Fi, then refresh the list.</span>
                </div>
              {/if}
            </section>

            <section
              class="override-section"
              aria-labelledby="override-heading"
            >
              <div class="advanced-section-heading">
                <div>
                  <h3 id="override-heading">Common overrides</h3>
                  <p>
                    Optional values replace paqet defaults for this session.
                  </p>
                </div>
              </div>

              {#if settingsOperation && isCommonSettingField(settingsOperation)}
                <p class="settings-progress" role="status" aria-live="polite">
                  Updating {settingsFieldLabel(settingsOperation)}…
                </p>
              {/if}
              {#if settingsMessage}
                <p class="inline-message" role="alert">{settingsMessage}</p>
              {/if}

              <div class="override-list">
                <div class="override-item">
                  <label class="override-toggle">
                    <input
                      type="checkbox"
                      checked={snapshot.advancedSettings.logLevel !== null}
                      disabled={!settingsEditable ||
                        saving ||
                        interfaceBusy ||
                        settingsOperation === 'logLevel'}
                      onchange={(event) =>
                        toggleCommonOverride(
                          'logLevel',
                          (event.currentTarget as HTMLInputElement).checked,
                        )}
                    />
                    <span>
                      <strong>Override log level</strong>
                      <small>Info remains required for connection status.</small
                      >
                    </span>
                  </label>
                  <div class="override-control">
                    <label for="log-level">Log level</label>
                    <select
                      id="log-level"
                      value={snapshot.advancedSettings.logLevel ?? 'info'}
                      disabled={snapshot.advancedSettings.logLevel === null ||
                        !settingsEditable ||
                        saving ||
                        interfaceBusy ||
                        settingsOperation === 'logLevel'}
                      onchange={selectLogLevel}
                    >
                      <option value="info">Info</option>
                      <option value="debug">Debug</option>
                    </select>
                  </div>
                </div>

                <div class="override-item">
                  <label class="override-toggle">
                    <input
                      type="checkbox"
                      checked={snapshot.advancedSettings.pcapSocketBuffer !==
                        null}
                      disabled={!settingsEditable ||
                        saving ||
                        interfaceBusy ||
                        settingsOperation === 'pcapSocketBuffer'}
                      onchange={(event) =>
                        toggleCommonOverride(
                          'pcapSocketBuffer',
                          (event.currentTarget as HTMLInputElement).checked,
                        )}
                    />
                    <span>
                      <strong>Override PCAP socket buffer</strong>
                      <small>Default 4194304 bytes.</small>
                    </span>
                  </label>
                  <div class="override-control">
                    <label for="pcap-socket-buffer">PCAP socket buffer</label>
                    <input
                      id="pcap-socket-buffer"
                      bind:this={commonFieldInputs.pcapSocketBuffer}
                      bind:value={commonDraft.pcapSocketBuffer}
                      inputmode="numeric"
                      autocomplete="off"
                      disabled={snapshot.advancedSettings.pcapSocketBuffer ===
                        null ||
                        !settingsEditable ||
                        saving ||
                        interfaceBusy}
                      aria-invalid={commonErrors.pcapSocketBuffer
                        ? 'true'
                        : undefined}
                      aria-describedby="pcap-socket-buffer-hint{commonErrors.pcapSocketBuffer
                        ? ' pcap-socket-buffer-error'
                        : ''}"
                      oninput={() => handleCommonInput('pcapSocketBuffer')}
                      onblur={() =>
                        scheduleCommonDraftCommit('pcapSocketBuffer')}
                      onkeydown={handleCommonKeydown}
                    />
                    <p class="field-hint" id="pcap-socket-buffer-hint">
                      1024–104857600 bytes
                    </p>
                    {#if commonErrors.pcapSocketBuffer}
                      <p class="field-error" id="pcap-socket-buffer-error">
                        {commonErrors.pcapSocketBuffer}
                      </p>
                    {/if}
                  </div>
                </div>

                {#each flagFields as field (field)}
                  {@const prefix =
                    field === 'localTcpFlags' ? 'Local' : 'Remote'}
                  {@const inputId =
                    field === 'localTcpFlags'
                      ? 'local-tcp-flags'
                      : 'remote-tcp-flags'}
                  <div class="override-item">
                    <label class="override-toggle">
                      <input
                        type="checkbox"
                        checked={snapshot.advancedSettings[field] !== null}
                        disabled={!settingsEditable ||
                          saving ||
                          interfaceBusy ||
                          settingsOperation === field}
                        onchange={(event) =>
                          toggleCommonOverride(
                            field,
                            (event.currentTarget as HTMLInputElement).checked,
                          )}
                      />
                      <span>
                        <strong
                          >Override {prefix.toLowerCase()} TCP flags</strong
                        >
                        <small>Default PA.</small>
                      </span>
                    </label>
                    <div class="override-control">
                      <label for={inputId}>{prefix} TCP flags</label>
                      <input
                        id={inputId}
                        bind:this={commonFieldInputs[field]}
                        bind:value={commonDraft[field]}
                        autocapitalize="characters"
                        autocomplete="off"
                        spellcheck="false"
                        disabled={snapshot.advancedSettings[field] === null ||
                          !settingsEditable ||
                          saving ||
                          interfaceBusy}
                        aria-invalid={commonErrors[field] ? 'true' : undefined}
                        aria-describedby="{inputId}-hint{commonErrors[field]
                          ? ` ${inputId}-error`
                          : ''}"
                        oninput={() => handleCommonInput(field)}
                        onblur={() => scheduleCommonDraftCommit(field)}
                        onkeydown={handleCommonKeydown}
                      />
                      <p class="field-hint" id="{inputId}-hint">
                        Comma-separated combinations, for example PA, S.
                      </p>
                      {#if commonErrors[field]}
                        <p class="field-error" id="{inputId}-error">
                          {commonErrors[field]}
                        </p>
                      {/if}
                    </div>
                  </div>
                {/each}

                {#each numericFields as item (item.field)}
                  {@const inputId = item.field.replace(
                    /[A-Z]/g,
                    (letter) => `-${letter.toLowerCase()}`,
                  )}
                  <div class="override-item">
                    <label class="override-toggle">
                      <input
                        type="checkbox"
                        checked={snapshot.advancedSettings[item.field] !== null}
                        disabled={!settingsEditable ||
                          saving ||
                          interfaceBusy ||
                          settingsOperation === item.field}
                        onchange={(event) =>
                          toggleCommonOverride(
                            item.field,
                            (event.currentTarget as HTMLInputElement).checked,
                          )}
                      />
                      <span>
                        <strong>Override {item.title}</strong>
                        <small>Default {item.defaultValue}.</small>
                      </span>
                    </label>
                    <div class="override-control">
                      <label for={inputId}>{item.label}</label>
                      <input
                        id={inputId}
                        bind:this={commonFieldInputs[item.field]}
                        bind:value={commonDraft[item.field]}
                        inputmode="numeric"
                        autocomplete="off"
                        disabled={snapshot.advancedSettings[item.field] ===
                          null ||
                          !settingsEditable ||
                          saving ||
                          interfaceBusy}
                        aria-invalid={commonErrors[item.field]
                          ? 'true'
                          : undefined}
                        aria-describedby="{inputId}-hint{commonErrors[
                          item.field
                        ]
                          ? ` ${inputId}-error`
                          : ''}"
                        oninput={() => handleCommonInput(item.field)}
                        onblur={() => scheduleCommonDraftCommit(item.field)}
                        onkeydown={handleCommonKeydown}
                      />
                      <p class="field-hint" id="{inputId}-hint">{item.hint}</p>
                      {#if commonErrors[item.field]}
                        <p class="field-error" id="{inputId}-error">
                          {commonErrors[item.field]}
                        </p>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            </section>

            <section
              class="override-section"
              aria-labelledby="transport-override-heading"
            >
              <div class="advanced-section-heading">
                <div>
                  <h3 id="transport-override-heading">
                    KCP and SMUX overrides
                  </h3>
                  <p>Transport tuning must match the remote paqet server.</p>
                </div>
              </div>

              {#if settingsOperation && isKcpSettingField(settingsOperation)}
                <p class="settings-progress" role="status" aria-live="polite">
                  Updating {settingsFieldLabel(settingsOperation)}…
                </p>
              {/if}
              {#if transportMessage}
                <p class="inline-message" role="alert">
                  {transportMessage}
                </p>
              {/if}

              <div class="override-list">
                <div class="override-item">
                  <label class="override-toggle">
                    <input
                      type="checkbox"
                      checked={snapshot.advancedSettings.kcpMode !== null}
                      disabled={!settingsEditable ||
                        saving ||
                        interfaceBusy ||
                        kcpModeQueued ||
                        settingsOperation === 'kcpMode'}
                      onchange={(event) =>
                        handleKcpOverrideToggle(event, 'kcpMode')}
                    />
                    <span>
                      <strong>Override KCP mode</strong>
                      <small>Default Fast.</small>
                    </span>
                  </label>
                  <div class="override-control">
                    <label for="kcp-mode">KCP mode</label>
                    <select
                      id="kcp-mode"
                      value={snapshot.advancedSettings.kcpMode ?? 'fast'}
                      disabled={snapshot.advancedSettings.kcpMode === null ||
                        !settingsEditable ||
                        saving ||
                        interfaceBusy ||
                        kcpModeQueued ||
                        settingsOperation === 'kcpMode'}
                      onchange={selectKcpMode}
                    >
                      <option value="normal">Normal</option>
                      <option value="fast">Fast</option>
                      <option value="fast2">Fast 2</option>
                      <option value="fast3">Fast 3</option>
                      <option value="manual">Manual</option>
                    </select>
                  </div>

                  {#if snapshot.advancedSettings.kcpMode === 'manual'}
                    <fieldset class="manual-kcp">
                      <legend>Manual KCP tuning</legend>
                      <p class="field-hint">
                        Initialized from the previously effective preset.
                      </p>
                      <div class="manual-kcp-grid">
                        {#each manualKcpFields as item (item.field)}
                          {@const inputId = item.field.replace(
                            /[A-Z]/g,
                            (letter) => `-${letter.toLowerCase()}`,
                          )}
                          <div class="field">
                            <label for={inputId}>{item.label}</label>
                            <input
                              id={inputId}
                              bind:this={kcpFieldInputs[item.field]}
                              bind:value={kcpDraft[item.field]}
                              inputmode="numeric"
                              autocomplete="off"
                              disabled={!settingsEditable ||
                                saving ||
                                interfaceBusy ||
                                kcpModeQueued ||
                                settingsOperation === 'kcpMode'}
                              aria-invalid={kcpErrors[item.field]
                                ? 'true'
                                : undefined}
                              aria-describedby="{inputId}-hint{kcpErrors[
                                item.field
                              ]
                                ? ` ${inputId}-error`
                                : ''}"
                              oninput={() => handleKcpInput(item.field)}
                              onblur={() => scheduleKcpDraftCommit(item.field)}
                              onkeydown={handleCommonKeydown}
                            />
                            <p class="field-hint" id="{inputId}-hint">
                              {item.hint}
                            </p>
                            {#if kcpErrors[item.field]}
                              <p class="field-error" id="{inputId}-error">
                                {kcpErrors[item.field]}
                              </p>
                            {/if}
                          </div>
                        {/each}
                      </div>
                      <div class="manual-boolean-list">
                        <label class="boolean-control">
                          <input
                            type="checkbox"
                            checked={snapshot.advancedSettings.manualKcp
                              .writeDelay ?? false}
                            disabled={!settingsEditable ||
                              saving ||
                              interfaceBusy ||
                              kcpModeQueued ||
                              settingsOperation === 'kcpMode' ||
                              settingsOperation === 'kcpWriteDelay'}
                            onchange={(event) =>
                              handleManualKcpBoolean(event, 'kcpWriteDelay')}
                          />
                          KCP write delay
                        </label>
                        <label class="boolean-control">
                          <input
                            type="checkbox"
                            checked={snapshot.advancedSettings.manualKcp
                              .ackNoDelay ?? false}
                            disabled={!settingsEditable ||
                              saving ||
                              interfaceBusy ||
                              kcpModeQueued ||
                              settingsOperation === 'kcpMode' ||
                              settingsOperation === 'kcpAckNoDelay'}
                            onchange={(event) =>
                              handleManualKcpBoolean(event, 'kcpAckNoDelay')}
                          />
                          KCP ACK nodelay
                        </label>
                      </div>
                    </fieldset>
                  {/if}
                </div>

                {#each kcpNumericFields as item (item.field)}
                  {@const inputId = item.field.replace(
                    /[A-Z]/g,
                    (letter) => `-${letter.toLowerCase()}`,
                  )}
                  <div class="override-item">
                    <label class="override-toggle">
                      <input
                        type="checkbox"
                        checked={snapshot.advancedSettings[item.field] !== null}
                        disabled={!settingsEditable ||
                          saving ||
                          interfaceBusy ||
                          settingsOperation === item.field}
                        onchange={(event) =>
                          handleKcpOverrideToggle(event, item.field)}
                      />
                      <span>
                        <strong>Override {item.title}</strong>
                        <small>Default {item.defaultValue}.</small>
                      </span>
                    </label>
                    <div class="override-control">
                      <label for={inputId}>{item.label}</label>
                      <input
                        id={inputId}
                        bind:this={kcpFieldInputs[item.field]}
                        bind:value={kcpDraft[item.field]}
                        inputmode="numeric"
                        autocomplete="off"
                        disabled={snapshot.advancedSettings[item.field] ===
                          null ||
                          !settingsEditable ||
                          saving ||
                          interfaceBusy}
                        aria-invalid={kcpErrors[item.field]
                          ? 'true'
                          : undefined}
                        aria-describedby="{inputId}-hint{kcpErrors[item.field]
                          ? ` ${inputId}-error`
                          : ''}"
                        oninput={() => handleKcpInput(item.field)}
                        onblur={() => scheduleKcpDraftCommit(item.field)}
                        onkeydown={handleCommonKeydown}
                      />
                      <p class="field-hint" id="{inputId}-hint">
                        {item.hint}
                      </p>
                      {#if kcpErrors[item.field]}
                        <p class="field-error" id="{inputId}-error">
                          {kcpErrors[item.field]}
                        </p>
                      {/if}
                    </div>
                  </div>
                {/each}

                <div class="override-item">
                  <label class="override-toggle">
                    <input
                      type="checkbox"
                      checked={snapshot.advancedSettings.kcpBlock !== null}
                      disabled={!settingsEditable ||
                        saving ||
                        interfaceBusy ||
                        settingsOperation === 'kcpBlock'}
                      onchange={(event) =>
                        handleKcpOverrideToggle(event, 'kcpBlock')}
                    />
                    <span>
                      <strong>Override KCP block</strong>
                      <small>Default AES; the server must match.</small>
                    </span>
                  </label>
                  <div class="override-control">
                    <label for="kcp-block">KCP block</label>
                    <select
                      id="kcp-block"
                      value={snapshot.advancedSettings.kcpBlock ?? 'aes'}
                      disabled={snapshot.advancedSettings.kcpBlock === null ||
                        !settingsEditable ||
                        saving ||
                        interfaceBusy ||
                        settingsOperation === 'kcpBlock'}
                      onchange={selectKcpBlock}
                    >
                      <option value="aes">AES</option>
                      <option value="aes-128-gcm">AES-128-GCM</option>
                      <option value="aes-128">AES-128</option>
                      <option value="aes-192">AES-192</option>
                      <option value="salsa20">Salsa20</option>
                      <option value="blowfish">Blowfish</option>
                      <option value="twofish">Twofish</option>
                      <option value="cast5">CAST5</option>
                      <option value="3des">3DES</option>
                      <option value="tea">TEA</option>
                      <option value="xtea">XTEA</option>
                      <option value="xor">XOR</option>
                      <option value="sm4">SM4</option>
                      <option value="none">None (insecure)</option>
                      <option value="null">Null (insecure)</option>
                    </select>
                    <p class="field-hint">
                      None and Null disable encryption and require confirmation.
                    </p>
                  </div>
                </div>
              </div>
            </section>
          </div>
        {/if}
      </div>
    {/if}
  </section>

  <section class="connection" aria-labelledby="connection-heading">
    {#if message}
      <p class="app-message" role="alert">{message}</p>
    {/if}
    {#if connectionMessage}
      <p class="app-message" role="alert">{connectionMessage}</p>
    {/if}
    {#if failureMessage && !connectionMessage}
      <p class="failure-message" role="status" aria-live="polite">
        {failureMessage}
      </p>
    {/if}
    <h2 id="connection-heading" class="sr-only">Connection</h2>
    <button
      class:disconnect-action={connectionAction.kind === 'disconnect'}
      class:connection-pending={connectionAction.kind === 'waiting'}
      class="connect-button"
      type="button"
      disabled={connectionDisabled}
      aria-busy={connectionBusy || connectionAction.kind === 'waiting'}
      onclick={runConnectionAction}
    >
      <span aria-hidden="true"></span>
      {connectionAction.label}
    </button>

    <div class="log-heading">
      <div>
        <p class="eyebrow">Session</p>
        <h2>Logs</h2>
      </div>
      <div class="log-actions" aria-label="Log actions">
        <button
          class="text-button"
          type="button"
          disabled={logEntries.length === 0}
          onclick={copyLogs}>Copy</button
        >
        <button
          class="text-button"
          type="button"
          disabled={logEntries.length === 0}
          onclick={clearLogs}>Clear</button
        >
      </div>
    </div>
    {#if copyMessage}
      <p
        class:copy-error={copyFailed}
        class="copy-message"
        role={copyFailed ? 'alert' : 'status'}
        aria-live="polite"
      >
        {copyMessage}
      </p>
    {/if}
    <div class="log-shell">
      <div
        class="log"
        bind:this={logElement}
        role="log"
        aria-label="Connection logs"
        aria-live="polite"
        aria-relevant="additions text"
        onscroll={handleLogScroll}
      >
        {#if logEntries.length === 0}
          <p>Connection output will appear here.</p>
        {:else}
          {#each logEntries as entry (logEntryKey(entry))}
            {#if entry.kind === 'gap'}
              <p class="log-gap">
                Output unavailable: sequences {entry.firstMissing}–{(
                  BigInt(entry.nextAvailable) - 1n
                ).toString()}.
              </p>
            {:else}
              <p
                class:log-stderr={entry.record.stream === 'stderr'}
                class="log-record"
              >
                {#if entry.record.stream === 'stderr'}
                  <span class="stream-marker">stderr</span>
                {/if}
                <span>{entry.record.text}</span>
                {#if entry.record.truncated}
                  <span class="truncated-marker">record truncated</span>
                {/if}
              </p>
            {/if}
          {/each}
        {/if}
      </div>
      {#if !followingLogs && logEntries.length > 0}
        <button class="jump-button" type="button" onclick={scrollLogToLatest}>
          Jump to latest
        </button>
      {/if}
    </div>
  </section>
</main>

{#if dialog}
  <div
    class="dialog-backdrop"
    role="presentation"
    onkeydown={handleDialogKeydown}
  >
    <div
      class="dialog"
      bind:this={dialogElement}
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="dialog-title"
      aria-describedby="dialog-description"
    >
      {#if dialog.kind === 'windowClose'}
        <p class="eyebrow">paqet is active</p>
        <h2 id="dialog-title">Disconnect and close?</h2>
        <p id="dialog-description">
          paqet is {formatStatus(
            dialog.request.lifecycle.status,
          ).toLowerCase()}. Closing will stop the supervised client process and
          wait for its process tree to exit.
        </p>
        {#if closeDecisionMessage}
          <p class="inline-message" role="alert">{closeDecisionMessage}</p>
        {/if}
        <div class="dialog-actions">
          <button
            class="text-button"
            type="button"
            bind:this={dialogCancelButton}
            disabled={closeDecisionBusy}
            onclick={cancelWindowClose}>Keep open</button
          >
          <button
            class="danger-button"
            type="button"
            bind:this={dialogPrimaryButton}
            disabled={closeDecisionBusy}
            onclick={confirmDialog}
          >
            {closeDecisionBusy ? 'Closing…' : 'Disconnect and close'}
          </button>
        </div>
      {:else if dialog.kind === 'delete'}
        <p class="eyebrow">Permanent action</p>
        <h2 id="dialog-title">Delete “{dialog.profile.name}”?</h2>
        <p id="dialog-description">
          This removes the saved server profile. This action cannot be undone.
        </p>
        <div class="dialog-actions">
          <button class="text-button" type="button" onclick={closeDialog}
            >Cancel</button
          >
          <button
            class="danger-button"
            type="button"
            bind:this={dialogPrimaryButton}
            onclick={confirmDialog}
          >
            Delete profile
          </button>
        </div>
      {:else if dialog.kind === 'unsafeBlock'}
        <p class="eyebrow">Security warning</p>
        <h2 id="dialog-title">
          Use insecure “{dialog.value}” KCP block?
        </h2>
        <p id="dialog-description">
          Traffic will not be encrypted or authenticated. The server must use
          this exact value; None and Null are not interchangeable.
        </p>
        <div class="dialog-actions">
          <button
            class="text-button"
            type="button"
            bind:this={dialogCancelButton}
            onclick={closeDialog}>Cancel</button
          >
          <button
            class="danger-button"
            type="button"
            bind:this={dialogPrimaryButton}
            onclick={confirmDialog}
          >
            Use insecure block
          </button>
        </div>
      {:else}
        <p class="eyebrow">Unsaved changes</p>
        <h2 id="dialog-title">Discard your changes?</h2>
        <p id="dialog-description">
          The profile edits you have made will not be saved.
        </p>
        <div class="dialog-actions">
          <button class="text-button" type="button" onclick={closeDialog}
            >Keep editing</button
          >
          <button
            class="primary-small"
            type="button"
            bind:this={dialogPrimaryButton}
            onclick={confirmDialog}
          >
            Discard changes
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}
