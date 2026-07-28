<script lang="ts">
  import { onMount, tick } from 'svelte';
  import * as tauriApi from './lib/api';
  import type {
    AppSnapshot,
    IpcError,
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
  }

  type EditorMode = 'view' | 'create' | 'edit';
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
  let revealKey = $state(false);
  let advancedExpanded = $state(false);
  let dialog = $state<DialogState>(null);

  let nameInput = $state<HTMLInputElement>();
  let serverHostInput = $state<HTMLInputElement>();
  let portInput = $state<HTMLInputElement>();
  let encryptionKeyInput = $state<HTMLInputElement>();
  let dialogPrimaryButton = $state<HTMLButtonElement>();
  let dialogElement = $state<HTMLDivElement>();
  let profileSelect = $state<HTMLSelectElement>();
  let newProfileButton = $state<HTMLButtonElement>();
  let dialogInvoker: HTMLElement | null = null;

  const selectedProfile = $derived(snapshot?.selectedProfile ?? null);
  const settingsEditable = $derived(
    snapshot?.lifecycle.settingsEditable ?? false,
  );
  const editorOpen = $derived(editorMode !== 'view');
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
      applySnapshot(await api.getAppSnapshot());
    } catch {
      message =
        'The application state could not be loaded. Restart paqet and try again.';
    } finally {
      loading = false;
    }
  }

  function applySnapshot(nextSnapshot: AppSnapshot): void {
    snapshot = nextSnapshot;
    editorMode = 'view';
    fieldErrors = {};
    revealKey = false;
    draft = nextSnapshot.selectedProfile
      ? profileInput(nextSnapshot.selectedProfile)
      : emptyProfileInput();
  }

  function beginCreate(): void {
    if (!settingsEditable || saving) return;
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
    if (!selectedProfile || !settingsEditable || saving) return;
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
      applySnapshot(await api.selectProfile(profileId));
    } catch {
      message = 'The selected profile could not be opened.';
    } finally {
      saving = false;
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
    if (!editorOpen || !settingsEditable || saving) return;

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
      applySnapshot(nextSnapshot);
    } catch (error) {
      saving = false;
      await presentProfileError(error);
      return;
    }
    saving = false;
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
    if (!selectedProfile || !settingsEditable || saving) return;
    openDialog({ kind: 'delete', profile: selectedProfile });
  }

  async function confirmDialog(): Promise<void> {
    const action = dialog;
    dialog = null;
    dialogInvoker = null;
    if (!action) return;

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
      applySnapshot(await api.deleteProfile(action.profile.id));
    } catch {
      message = `The profile “${action.profile.name}” could not be deleted.`;
    } finally {
      saving = false;
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
            saving ||
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
          disabled={!settingsEditable || saving}
          onclick={beginCreate}
        >
          New
        </button>
        <button
          class="secondary-button compact-button"
          type="button"
          disabled={!selectedProfile ||
            !settingsEditable ||
            saving ||
            editorOpen}
          onclick={beginEdit}
        >
          Edit
        </button>
      </div>

      {#if !selectedProfile && editorMode === 'view'}
        <div class="empty-state">
          <p>Add a server profile to begin configuring paqet.</p>
          <button class="secondary-button" type="button" onclick={beginCreate}>
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
              readonly={!editorOpen}
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
                readonly={!editorOpen}
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
                readonly={!editorOpen}
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
                readonly={!editorOpen}
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
                  disabled={saving}
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
              <button class="primary-small" type="submit" disabled={saving}>
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
            <p>
              Network interface details and optional paqet overrides appear here
              when configured.
            </p>
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
