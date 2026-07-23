<svelte:head>
  <meta
    name="description"
    content="A lightweight Windows desktop client for paqet."
  />
</svelte:head>

<main class="app-shell">
  <header class="topbar">
    <div>
      <p class="eyebrow">Windows client</p>
      <h1>paqet</h1>
    </div>
    <p class="status" aria-label="Connection status">Disconnected</p>
  </header>

  <section class="setup" aria-labelledby="setup-title">
    <div>
      <p class="eyebrow">Local setup</p>
      <h2 id="setup-title">Client foundation ready</h2>
    </div>
    <p class="summary">
      Server profiles, network selection, and paqet process controls will be
      added in focused implementation steps.
    </p>
    <dl class="readiness-list">
      <div>
        <dt>Application shell</dt>
        <dd>Available</dd>
      </div>
      <div>
        <dt>Configuration</dt>
        <dd>Not configured</dd>
      </div>
      <div>
        <dt>Runtime</dt>
        <dd>Idle</dd>
      </div>
    </dl>
  </section>

  <section class="connection" aria-labelledby="connection-title">
    <div class="connection-heading">
      <div>
        <p class="eyebrow">Connection</p>
        <h2 id="connection-title">Waiting for setup</h2>
      </div>
      <span class="signal" aria-hidden="true"></span>
    </div>

    <button type="button" disabled>Connect</button>

    <div class="log-heading">
      <h2>Logs</h2>
      <span>Session output</span>
    </div>
    <div class="log" role="log" aria-label="Connection logs">
      <p>Connection output will appear here.</p>
    </div>
  </section>
</main>
