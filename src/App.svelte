<script lang="ts">
  import { onMount, tick } from 'svelte';
  import * as tauriApi from './lib/api';
  import type {
    AdvancedSettings,
    AppSnapshot,
    IpcError,
    LogLevel,
    NetworkInterface,
    Profile,
    ProfileDraft,
    ProfileFieldName,
    ProfileId,
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
  type ProfileInput = Omit<ProfileDraft, 'port'> & { port: string };
  type FieldErrors = Partial<Record<ProfileFieldName, string>>;
  type DialogState =
    | { kind: 'discardSelection'; profileId: ProfileId }
    | { kind: 'discardCreate' }
    | { kind: 'delete'; profile: Profile }
    | null;

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
  let settingsOperation = $state<CommonSettingField | null>(null);
  let settingsMessage = $state('');
  let commonDraft = $state<CommonDraft>(defaultCommonDraft());
  let commonErrors = $state<CommonErrors>({});
  let commonDraftVersions = $state<Record<CommonTextField, number>>(
    initialCommonDraftVersions(),
  );
  let revealKey = $state(false);
  let advancedExpanded = $state(false);
  let dialog = $state<DialogState>(null);

  let nameInput = $state<HTMLInputElement>();
  let serverHostInput = $state<HTMLInputElement>();
  let portInput = $state<HTMLInputElement>();
  let encryptionKeyInput = $state<HTMLInputElement>();
  let commonFieldInputs: Partial<Record<CommonTextField, HTMLInputElement>> =
    {};
  let dialogPrimaryButton = $state<HTMLButtonElement>();
  let dialogElement = $state<HTMLDivElement>();
  let profileSelect = $state<HTMLSelectElement>();
  let newProfileButton = $state<HTMLButtonElement>();
  let dialogInvoker: HTMLElement | null = null;
  let mutationIdleResolvers: Array<() => void> = [];
  let settingsQueue = Promise.resolve();

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

  onMount(() => {
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
        'The application state could not be loaded. Restart paqet and try again.';
    } finally {
      loading = false;
    }
  }

  function applySnapshot(
    nextSnapshot: AppSnapshot,
    resetProfileEditor = false,
    commonFieldToSync: CommonSettingField | 'all' | null = null,
  ): boolean {
    if (snapshot && BigInt(nextSnapshot.revision) < BigInt(snapshot.revision)) {
      return false;
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
    if (commonFieldToSync) {
      syncCommonDraft(nextSnapshot.advancedSettings, commonFieldToSync);
    }
    return true;
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
    dialog = null;
    dialogInvoker = null;
    if (!action) return;

    await waitForMutationIdle();
    if (!settingsEditable) return;

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
    void tick().then(() => dialogPrimaryButton?.focus());
  }

  function closeDialog(): void {
    const returnFocus = dialogInvoker;
    dialog = null;
    dialogInvoker = null;
    void tick().then(() => returnFocus?.focus());
  }

  function handleDialogKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      closeDialog();
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
    window.setTimeout(() => void commitCommonDraft(field), 0);
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
    content="A lightweight Windows desktop client for paqet."
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
      <h1>paqet</h1>
    </div>
    <p
      class:status-failed={statusLabel === 'Failed'}
      class:status-connected={statusLabel === 'Connected'}
      class="status"
      aria-label="Connection status"
      aria-live="polite"
    >
      <span aria-hidden="true"></span>
      {statusLabel}
    </p>
  </header>

  <section class="configuration" aria-labelledby="profile-heading">
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

              {#if settingsOperation}
                <p class="settings-progress" role="status" aria-live="polite">
                  Updating {commonSettingLabel(settingsOperation)}…
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
          </div>
        {/if}
      </div>
    {/if}
  </section>

  <section class="connection" aria-labelledby="connection-heading">
    {#if message}
      <p class="app-message" role="alert">{message}</p>
    {/if}
    <h2 id="connection-heading" class="sr-only">Connection</h2>
    <button class="connect-button" type="button" disabled>
      <span aria-hidden="true"></span>
      Connect
    </button>

    <div class="log-heading">
      <div>
        <p class="eyebrow">Session</p>
        <h2>Logs</h2>
      </div>
      <div class="log-actions" aria-label="Log actions">
        <button class="text-button" type="button" disabled>Copy</button>
        <button class="text-button" type="button" disabled>Clear</button>
      </div>
    </div>
    <div class="log" role="log" aria-label="Connection logs">
      <p>Connection output will appear here.</p>
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
      {#if dialog.kind === 'delete'}
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
