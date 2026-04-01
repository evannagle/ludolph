// ludolph.dev — client-side app
// Fetches registry data and renders vault/plugin/jetpack directories.

const REGISTRY_BASE = 'https://raw.githubusercontent.com/evannagle/ludolph-registry/main';
const REGISTRY_REPO = 'evannagle/ludolph-registry';

let _registryCache = null;

async function fetchRegistry() {
  if (_registryCache) return _registryCache;

  try {
    const response = await fetch(`${REGISTRY_BASE}/index.json`);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    _registryCache = await response.json();
    return _registryCache;
  } catch (e) {
    console.warn('Failed to fetch registry:', e);
    return { vaults: [], plugins: [], jetpacks: [] };
  }
}

// --- Rendering ---

function renderVaultCard(vault) {
  const v = vault.vault || {};
  const k = vault.knowledge || {};
  const topics = (k.topics || []).slice(0, 5);
  const stats = k.stats || {};

  return `
    <a href="vault.html?owner=${encodeURIComponent(v.owner)}" class="card">
      <h3>${esc(v.name)}</h3>
      <div class="owner">@${esc(v.owner)}</div>
      <div class="description">${esc(v.description)}</div>
      <div class="stats">${stats.total_chunks || 0} chunks from ${stats.total_sources || 0} sources</div>
      <div class="tags">${topics.map(t => `<span class="tag">${esc(t)}</span>`).join('')}</div>
    </a>
  `;
}

function renderPluginCard(plugin) {
  return `
    <div class="card">
      <h3>${esc(plugin.name)}</h3>
      <div class="description">${esc(plugin.description || '')}</div>
      <div class="stats">${plugin.package ? `Package: ${esc(plugin.package)}` : ''}</div>
      <div class="tags">
        ${(plugin.env_vars || []).map(v => `<span class="tag">${esc(v)}</span>`).join('')}
      </div>
    </div>
  `;
}

function renderJetpackCard(jetpack) {
  return `
    <div class="card">
      <h3>${esc(jetpack.name)}</h3>
      <div class="description">${esc(jetpack.description || '')}</div>
      ${jetpack.mcps ? `<div class="stats">Requires: ${jetpack.mcps.join(', ')}</div>` : ''}
    </div>
  `;
}

function renderNeighborhood(vault, allVaults) {
  const topics = new Set((vault.knowledge?.topics || []));
  if (topics.size === 0) return '';

  const neighbors = [];
  for (const other of allVaults) {
    if (other.vault?.owner === vault.vault?.owner) continue;
    const otherTopics = other.knowledge?.topics || [];
    const shared = otherTopics.filter(t => topics.has(t));
    if (shared.length > 0) {
      neighbors.push({ vault: other, shared });
    }
  }

  neighbors.sort((a, b) => b.shared.length - a.shared.length);

  if (neighbors.length === 0) return '<div class="empty">No neighbors yet.</div>';

  return neighbors.slice(0, 5).map(n => `
    <div class="neighborhood">
      <a href="vault.html?owner=${encodeURIComponent(n.vault.vault?.owner)}">${esc(n.vault.vault?.name)}</a>
      <span class="shared">${n.shared.map(t => esc(t)).join(', ')}</span>
    </div>
  `).join('');
}

function buildLearnRequestUrl(owner, topic) {
  const title = `Learn request: ${topic} from @${owner}`;
  const body = `I'd like to learn about **${topic}** from @${owner}'s vault.\n\nPlease share what you can at a privacy tier you're comfortable with.`;
  return `https://github.com/${REGISTRY_REPO}/issues/new?title=${encodeURIComponent(title)}&labels=learn-request&body=${encodeURIComponent(body)}`;
}

// --- Utilities ---

function esc(str) {
  if (!str) return '';
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}

function getParam(name) {
  return new URLSearchParams(window.location.search).get(name);
}

// --- Page Initializers ---

async function initVaults() {
  const registry = await fetchRegistry();
  const container = document.getElementById('vault-grid');
  const search = document.getElementById('vault-search');
  const vaults = registry.vaults || [];

  function render(filter) {
    const filtered = filter
      ? vaults.filter(v => {
          const text = JSON.stringify(v).toLowerCase();
          return text.includes(filter.toLowerCase());
        })
      : vaults;

    container.innerHTML = filtered.length
      ? filtered.map(renderVaultCard).join('')
      : '<div class="empty">No vaults found.</div>';
  }

  render();
  if (search) {
    search.addEventListener('input', () => render(search.value));
  }
}

async function initVaultProfile() {
  const owner = getParam('owner');
  if (!owner) {
    document.getElementById('profile').innerHTML = '<div class="empty">No vault specified.</div>';
    return;
  }

  const registry = await fetchRegistry();
  const vaults = registry.vaults || [];
  const vault = vaults.find(v => v.vault?.owner === owner);

  if (!vault) {
    document.getElementById('profile').innerHTML = `<div class="empty">Vault @${esc(owner)} not found.</div>`;
    return;
  }

  const v = vault.vault || {};
  const k = vault.knowledge || {};
  const stats = k.stats || {};
  const topics = k.topics || [];
  const queries = vault.sample_queries || [];
  const privacy = vault.privacy || {};

  let html = `
    <div class="profile-header">
      <h1>${esc(v.name)}</h1>
      <div class="owner">@${esc(v.owner)}</div>
      <p>${esc(v.description)}</p>
    </div>

    <div class="profile-section">
      <h2>Knowledge</h2>
      <div class="stat-grid">
        <div class="stat-box"><div class="number">${stats.total_chunks || 0}</div><div class="label">Chunks</div></div>
        <div class="stat-box"><div class="number">${stats.total_sources || 0}</div><div class="label">Sources</div></div>
        <div class="stat-box"><div class="number">${topics.length}</div><div class="label">Topics</div></div>
      </div>
      <div class="tags">${topics.map(t => `<span class="tag">${esc(t)}</span>`).join('')}</div>
    </div>
  `;

  if (queries.length > 0) {
    html += `
      <div class="profile-section">
        <h2>Sample Queries</h2>
        <ul>${queries.map(q => `<li style="margin-bottom: 8px; color: var(--text-secondary)">${esc(q)}</li>`).join('')}</ul>
      </div>
    `;
  }

  if (privacy.accepts_requests && topics.length > 0) {
    html += `
      <div class="profile-section">
        <h2>Request to Learn</h2>
        <p style="margin-bottom: 16px; color: var(--text-secondary)">Ask @${esc(v.owner)} to teach you about a topic.</p>
        <div class="tags">
          ${topics.map(t => `<a href="${buildLearnRequestUrl(v.owner, t)}" class="btn" target="_blank">${esc(t)}</a>`).join(' ')}
        </div>
      </div>
    `;
  }

  // Neighborhoods
  html += `
    <div class="profile-section">
      <h2>Knowledge Neighborhoods</h2>
      ${renderNeighborhood(vault, vaults)}
    </div>
  `;

  document.getElementById('profile').innerHTML = html;
}

async function initPlugins() {
  const registry = await fetchRegistry();
  const container = document.getElementById('plugin-grid');
  const plugins = registry.plugins || [];

  container.innerHTML = plugins.length
    ? plugins.map(renderPluginCard).join('')
    : '<div class="empty">No plugins published yet.</div>';
}

async function initJetpacks() {
  const registry = await fetchRegistry();
  const container = document.getElementById('jetpack-grid');
  const jetpacks = registry.jetpacks || [];

  container.innerHTML = jetpacks.length
    ? jetpacks.map(renderJetpackCard).join('')
    : '<div class="empty">No jetpacks published yet. Check the <a href="https://github.com/evannagle/ludolph/blob/develop/docs/jetpacks.md">jetpacks guide</a>.</div>';
}

async function initHome() {
  const registry = await fetchRegistry();
  const vaults = registry.vaults || [];
  const plugins = registry.plugins || [];
  const jetpacks = registry.jetpacks || [];

  const statsEl = document.getElementById('home-stats');
  if (statsEl) {
    statsEl.innerHTML = `
      <div class="stat-grid">
        <div class="stat-box"><div class="number">${vaults.length}</div><div class="label">Vaults</div></div>
        <div class="stat-box"><div class="number">${plugins.length}</div><div class="label">Plugins</div></div>
        <div class="stat-box"><div class="number">${jetpacks.length}</div><div class="label">Jetpacks</div></div>
      </div>
    `;
  }

  const featuredEl = document.getElementById('featured-vaults');
  if (featuredEl && vaults.length > 0) {
    featuredEl.innerHTML = `<div class="card-grid">${vaults.slice(0, 3).map(renderVaultCard).join('')}</div>`;
  }
}
