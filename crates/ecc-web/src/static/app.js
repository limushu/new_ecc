// ecc Dashboard — Route management, usage stats, playground

var API = '';

// --- Theme ---
(function() {
  var theme = localStorage.getItem('ecc-theme') || 'dark';
  document.documentElement.setAttribute('data-theme', theme);
})();
function toggleTheme() {
  var el = document.documentElement;
  var next = el.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
  el.setAttribute('data-theme', next);
  localStorage.setItem('ecc-theme', next);
}

// --- Tab switching ---
function switchTab(tabName) {
  document.querySelectorAll('.tab').forEach(function(t) { t.classList.remove('active'); });
  document.querySelectorAll('.tab-panel').forEach(function(p) { p.classList.remove('active'); });
  var btn = document.querySelector('.tab[data-tab="' + tabName + '"]');
  if (btn) btn.classList.add('active');
  var panel = document.getElementById('panel-' + tabName);
  if (panel) panel.classList.add('active');
  location.hash = tabName;
  if (tabName === 'routes') loadRoutes();
  if (tabName === 'usage' && window._buildCharts) window._buildCharts();
  if (tabName === 'playground') pgLoadModels();
}

document.querySelectorAll('.tab').forEach(function(btn) {
  btn.addEventListener('click', function() {
    switchTab(btn.dataset.tab);
  });
});

// Restore tab from URL hash on load (moved to end so _buildCharts is defined)

// --- Toast ---
function toast(msg, type) {
  var el = document.createElement('div');
  el.className = 'toast toast-' + (type || 'success');
  el.textContent = msg;
  document.getElementById('toast-container').appendChild(el);
  setTimeout(function() {
    el.classList.add('toast-out');
    setTimeout(function() { el.remove(); }, 300);
  }, 3000);
}

// --- Template data ---
var templateData = [];

function loadTemplates() {
  fetch(API + '/api/presets').then(function(r) { return r.json(); }).then(function(data) {
    templateData = data.presets || [];
    populateTemplateSelect();
  });
}

function populateTemplateSelect() {
  var sel = document.getElementById('modal-template');
  sel.innerHTML = '<option value="">Choose a provider template…</option>';
  templateData.forEach(function(t) {
    sel.innerHTML += '<option value="' + esc(t.name) + '">' + esc(t.name) + '</option>';
  });
}

function findTemplate(name) {
  for (var i = 0; i < templateData.length; i++) {
    if (templateData[i].name === name) return templateData[i];
  }
  return null;
}

// --- Routes ---
var allProviders = {};
var allRoutes = {};
var CARD_COLORS = ['#818cf8', '#34d399', '#fbbf24', '#f472b6', '#38bdf8', '#fb923c', '#a78bfa', '#2dd4bf'];

function loadRoutes() {
  Promise.all([
    fetch(API + '/api/providers').then(function(r) { return r.json(); }),
    fetch(API + '/api/routes').then(function(r) { return r.json(); })
  ]).then(function(results) {
    allProviders = (results[0].providers || {});
    allRoutes = (results[1].routes || {});
    renderProviderCards();
  }).catch(function(e) {
    console.error('loadRoutes failed:', e);
  });
}

function renderProviderCards() {
  var container = document.getElementById('provider-cards');
  container.innerHTML = '';

  // Group routes by provider
  var providerRoutes = {};
  for (var model in allRoutes) {
    var entry = allRoutes[model];
    (entry.targets || []).forEach(function(t) {
      if (!providerRoutes[t.provider]) providerRoutes[t.provider] = [];
      providerRoutes[t.provider].push({ claudeModel: model, targetModel: t.model });
    });
  }

  var sortedProviders = Object.keys(providerRoutes).sort();
  if (sortedProviders.length === 0) {
    container.innerHTML = '<div class="route-empty">No routes configured. Click "+ Add Route" to create one.</div>';
    return;
  }

  sortedProviders.forEach(function(pn, pi) {
    var entries = providerRoutes[pn];
    var provInfo = allProviders[pn] || {};
    var proto = provInfo.protocol || 'anthropic';
    var color = CARD_COLORS[pi % CARD_COLORS.length];

    var card = document.createElement('div');
    card.className = 'provider-card';
    card.style.borderTopColor = color;

    var head = document.createElement('div');
    head.className = 'provider-card-head';
    head.innerHTML =
      '<div class="provider-card-name">' + esc(pn) + ' <span class="tag tag-protocol">' + esc(proto) + '</span></div>' +
      '<span class="icon-bar">' +
      '<button class="icon-btn icon-btn-edit" title="Edit provider" onclick="editProvider(\'' + escJs(pn) + '\')">' +
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>' +
      '</button>' +
      '<button class="icon-btn" title="Delete provider" onclick="deleteProvider(\'' + escJs(pn) + '\')">' +
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>' +
      '</button></span>';

    var body = document.createElement('div');
    body.className = 'provider-card-body';

    entries.sort(function(a, b) { return a.claudeModel.localeCompare(b.claudeModel); });
    entries.forEach(function(e) {
      var item = document.createElement('div');
      item.className = 'route-item';
      item.innerHTML =
        '<div class="route-models">' +
        '<span>' + esc(e.claudeModel) + '</span>' +
        '<span class="route-arrow">&rarr;</span>' +
        '<span class="route-target">' + esc(e.targetModel) + '</span>' +
        '</div>' +
        '<span class="icon-bar">' +
        '<button class="icon-btn icon-btn-chart" title="Usage detail" onclick="showModelDetail(\'' + escJs(pn) + '\',\'' + escJs(e.targetModel) + '\')">' +
        '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="20" x2="18" y2="10"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="6" y1="20" x2="6" y2="14"/></svg>' +
        '</button>' +
        '<button class="icon-btn icon-btn-edit" title="Edit route" onclick="editRoute(\'' + escJs(e.claudeModel) + '\',\'' + escJs(pn) + '\',\'' + escJs(e.targetModel) + '\')">' +
        '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>' +
        '</button>' +
        '<button class="icon-btn" title="Delete route" onclick="deleteRoute(\'' + escJs(e.claudeModel) + '\')">' +
        '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>' +
        '</button></span>';
      body.appendChild(item);
    });

    var addBtn = document.createElement('div');
    addBtn.className = 'provider-card-add';
    addBtn.innerHTML = '<button class="provider-add-btn" onclick="addRouteTo(\'' + escJs(pn) + '\')">+ Add Model</button>';

    var quotaDiv = document.createElement('div');
    quotaDiv.className = 'quota-section';
    quotaDiv.id = 'quota-' + escJs(pn);

    card.appendChild(head);
    card.appendChild(body);
    card.appendChild(quotaDiv);
    card.appendChild(addBtn);
    container.appendChild(card);
  });

  paginateCards();

  // Batch load all quotas in one request
  loadAllQuotas();
}

// --- Quota ---
var quotaCache = {};

function loadAllQuotas() {
  fetch(API + '/api/quota').then(function(r) { return r.json(); }).then(function(data) {
    quotaCache = data;
    for (var name in data) {
      var el = document.getElementById('quota-' + name);
      if (el) renderQuota(el, data[name]);
    }
  }).catch(function() {});
}

function renderQuota(el, data) {
  if (!data.success && data.error && data.error.indexOf('does not support') !== -1) {
    el.style.display = 'none';
    return;
  }
  if (!data.success) {
    el.innerHTML = '<span class="quota-err">' + esc(data.error || 'Query failed') + '</span>';
    return;
  }
  var tiers = data.tiers || [];
  if (!tiers.length) { el.style.display = 'none'; return; }

  var html = '';
  // Tier theme colors: 5h = cyan/teal, Weekly = purple/violet
  var tierThemes = {
    five_hour: { ring: '#22d3ee', glow: 'rgba(34,211,238,0.35)', label: '5h' },
    weekly_limit: { ring: '#818cf8', glow: 'rgba(129,140,248,0.35)', label: 'Weekly' }
  };
  tiers.forEach(function(t) {
    var pct = Math.round(t.utilization);
    var theme = tierThemes[t.name] || { ring: '#818cf8', glow: 'rgba(129,140,248,0.35)', label: t.name };
    // Override color when usage is high
    var strokeColor = theme.ring;
    var glowColor = theme.glow;
    if (pct >= 90) { strokeColor = '#f87171'; glowColor = 'rgba(248,113,113,0.4)'; }
    else if (pct >= 70) { strokeColor = '#fbbf24'; glowColor = 'rgba(251,191,36,0.35)'; }
    var C = 113.1;
    var dash = C * pct / 100;
    var resetText = '';
    if (t.resets_at) {
      var diffMs = new Date(t.resets_at).getTime() - Date.now();
      if (diffMs > 0) {
        var h = Math.floor(diffMs / 3600000);
        var m = Math.floor((diffMs % 3600000) / 60000);
        if (h > 24) { resetText = Math.floor(h / 24) + 'd' + (h % 24) + 'h'; }
        else if (h > 0) { resetText = h + 'h' + m + 'm'; }
        else { resetText = m + 'm'; }
      }
    }
    html += '<div class="quota-ring-wrap">' +
      '<svg viewBox="0 0 40 40">' +
      '<circle class="quota-ring-bg" cx="20" cy="20" r="18"/>' +
      '<circle class="quota-ring-fill" cx="20" cy="20" r="18" stroke="' + strokeColor + '" style="drop-shadow:0 0 4px ' + glowColor + '" stroke-dasharray="' + dash + ' ' + C + '"/>' +
      '</svg>' +
      '<div class="quota-ring-pct" style="color:' + strokeColor + '">' + pct + '%</div></div>' +
      '<div class="quota-info"><div class="quota-tier-name" style="color:' + theme.ring + '">' + theme.label + '</div>' +
      (resetText ? '<div class="quota-tier-reset">' + resetText + '</div>' : '') + '</div>';
  });
  el.innerHTML = html;
}

function deleteProvider(name) {
  if (!confirm('Delete provider "' + name + '" and all its routes?')) return;
  fetch(API + '/api/providers/' + encodeURIComponent(name), { method: 'DELETE' })
    .then(function(r) {
      if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
      toast('Provider "' + name + '" deleted');
      loadRoutes();
    })
    .catch(function(e) { toast('Error: ' + e.message, 'error'); });
}

// --- Edit Provider ---
function editProvider(name) {
  var p = allProviders[name] || {};
  document.getElementById('ep-name').value = name;
  document.getElementById('ep-url').value = p.base_url || '';
  document.getElementById('ep-token').value = p.auth_token || '';
  document.getElementById('ep-protocol').value = p.protocol || 'openai';
  document.getElementById('ep-auth').value = p.auth_type || 'bearer';
  document.getElementById('edit-provider-modal').style.display = 'flex';
}

function closeEditProvider() {
  document.getElementById('edit-provider-modal').style.display = 'none';
}

function submitEditProvider() {
  var name = document.getElementById('ep-name').value;
  var data = {};
  var url = document.getElementById('ep-url').value.trim();
  var token = document.getElementById('ep-token').value.trim();
  var proto = document.getElementById('ep-protocol').value;
  var auth = document.getElementById('ep-auth').value;
  if (url) data.base_url = url;
  if (token) data.auth_token = token;
  data.protocol = proto;
  data.auth_type = auth;
  fetch(API + '/api/providers/' + encodeURIComponent(name), {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(data)
  }).then(function(r) {
    if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
    toast('Provider "' + name + '" updated');
    closeEditProvider();
    loadRoutes();
  }).catch(function(e) { toast('Error: ' + e.message, 'error'); });
}

// --- Edit Route ---
function editRoute(model, provider, targetModel) {
  document.getElementById('er-orig-model').value = model;
  document.getElementById('er-claude').value = model;
  document.getElementById('er-provider').value = provider;
  document.getElementById('er-target').value = targetModel;
  document.getElementById('edit-route-modal').style.display = 'flex';
}

function closeEditRoute() {
  document.getElementById('edit-route-modal').style.display = 'none';
}

function submitEditRoute() {
  var origModel = document.getElementById('er-orig-model').value;
  var newModel = document.getElementById('er-claude').value.trim();
  var provider = document.getElementById('er-provider').value;
  var newTarget = document.getElementById('er-target').value.trim();
  if (!newModel || !newTarget) { toast('All fields are required', 'error'); return; }
  // Delete old route, create new one
  fetch(API + '/api/routes/' + encodeURIComponent(origModel), { method: 'DELETE' })
    .then(function(r) {
      if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
      return fetch(API + '/api/routes', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ model: newModel, provider: provider, target_model: newTarget, priority: 1 })
      });
    }).then(function(r) {
      if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
      toast('Route updated');
      closeEditRoute();
      loadRoutes();
    }).catch(function(e) { toast('Error: ' + e.message, 'error'); });
}

// --- Model Detail (placeholder for #3) ---
function showModelDetail(provider, model) {
  toast('Detail view for ' + provider + ' / ' + model + ' — coming soon', 'error');
}

function deleteRoute(model) {
  if (!confirm('Delete route for "' + model + '"?')) return;
  fetch(API + '/api/routes/' + encodeURIComponent(model), { method: 'DELETE' })
    .then(function(r) {
      if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
      toast('Route for "' + model + '" deleted');
      loadRoutes();
    })
    .catch(function(e) { toast('Error: ' + e.message, 'error'); });
}

// --- Card pagination ---
var PER_PAGE = 3;
function paginateCards() {
  var cards = document.querySelectorAll('.provider-card');
  cards.forEach(function(card) {
    var body = card.querySelector('.provider-card-body');
    var items = body.querySelectorAll('.route-item');
    var total = items.length;
    body.style.height = (PER_PAGE * 36 + 4) + 'px';
    if (total <= PER_PAGE) { ensureDots(card, 0); return; }
    var page = parseInt(card.dataset.page || '0');
    var pages = Math.ceil(total / PER_PAGE);
    if (page >= pages) page = 0;
    items.forEach(function(item, i) {
      item.style.display = (Math.floor(i / PER_PAGE) === page) ? '' : 'none';
    });
    ensureDots(card, pages, page);
  });
}
function ensureDots(card, pages, activePage) {
  var dots = card.querySelector('.card-dots');
  if (!dots) {
    dots = document.createElement('div');
    dots.className = 'card-dots';
    var addBtn = card.querySelector('.provider-card-add');
    if (addBtn) card.insertBefore(dots, addBtn);
    else card.appendChild(dots);
  }
  if (pages <= 1) { dots.innerHTML = ''; return; }
  dots.innerHTML = '';
  for (var p = 0; p < pages; p++) {
    var d = document.createElement('button');
    d.className = 'page-dot' + (p === activePage ? ' active' : '');
    (function(pp) { d.onclick = function() { card.dataset.page = pp; paginateCards(); }; })(p);
    dots.appendChild(d);
  }
}

// --- Add Route Modal ---
function openRouteModal() {
  document.getElementById('modal-title').textContent = 'Add Route';
  document.getElementById('modal-submit').textContent = 'Add Route';
  document.getElementById('modal-template').value = '';
  document.getElementById('modal-provider').value = '';
  document.getElementById('modal-claude-input').value = '';
  document.getElementById('modal-target-input').value = '';
  document.getElementById('modal-claude-select').value = '';
  document.getElementById('modal-target-select').value = '';
  document.getElementById('modal-new-provider').style.display = 'none';
  // Reset new-provider form
  document.getElementById('modal-new-name').value = '';
  document.getElementById('modal-new-url').value = '';
  document.getElementById('modal-new-token').value = '';
  document.getElementById('modal-new-protocol').value = 'openai';
  document.getElementById('modal-new-auth').value = 'bearer';
  populateProviderSelect();
  populateTemplateSelect();
  document.getElementById('route-modal').style.display = 'flex';
}

function addRouteTo(provider) {
  openRouteModal();
  document.getElementById('modal-provider').value = provider;
  onProviderChange();
}

function closeModal() {
  document.getElementById('route-modal').style.display = 'none';
}

function populateProviderSelect() {
  var sel = document.getElementById('modal-provider');
  sel.innerHTML = '<option value="">Select provider</option>';
  for (var name in allProviders) {
    sel.innerHTML += '<option value="' + esc(name) + '">' + esc(name) + '</option>';
  }
  sel.innerHTML += '<option value="__new__">+ New Provider</option>';
}

function onProviderChange() {
  var prov = document.getElementById('modal-provider').value;
  document.getElementById('modal-new-provider').style.display = (prov === '__new__') ? 'block' : 'none';
}

// Template change: fill new-provider form fields from preset
function onTemplateChange() {
  var name = document.getElementById('modal-template').value;
  if (!name) return;
  var tmpl = findTemplate(name);
  if (!tmpl) return;

  // Fill the new-provider form fields
  document.getElementById('modal-new-name').value = tmpl.name.toLowerCase();
  document.getElementById('modal-new-url').value = tmpl.base_url;
  document.getElementById('modal-new-protocol').value = tmpl.protocol;
  document.getElementById('modal-new-auth').value = tmpl.auth_type;

  // Populate target model dropdown from template
  populateTargetModels(tmpl, null);
}

function populateClaudeModels(tmpl) {
  var sel = document.getElementById('modal-claude-select');
  // Add suggested Claude models from template
  var seen = {};
  sel.querySelectorAll('option').forEach(function(o) {
    if (o.value) seen[o.value] = true;
  });
  (tmpl.suggested_mappings || []).forEach(function(m) {
    if (!seen[m.claude_model]) {
      seen[m.claude_model] = true;
      sel.innerHTML += '<option value="' + esc(m.claude_model) + '">' + esc(m.claude_model) + '</option>';
    }
  });
}

function populateTargetModels(tmpl, claudeModel) {
  var sel = document.getElementById('modal-target-select');
  sel.innerHTML = '<option value="">Select or type below</option>';
  var seen = {};

  // If a specific Claude model is selected, show its suggested provider models first
  if (claudeModel) {
    (tmpl.suggested_mappings || []).forEach(function(m) {
      if (m.claude_model === claudeModel) {
        m.provider_models.forEach(function(pm) {
          if (!seen[pm]) { seen[pm] = true; sel.innerHTML += '<option value="' + esc(pm) + '">' + esc(pm) + '</option>'; }
        });
      }
    });
  }

  // Also list all provider models from the template
  (tmpl.models || []).forEach(function(m) {
    if (!seen[m.id]) { seen[m.id] = true; sel.innerHTML += '<option value="' + esc(m.id) + '">' + esc(m.id) + ' (' + esc(m.name) + ')</option>'; }
  });
}

function syncField(sid, iid) {
  var v = document.getElementById(sid).value;
  if (v) document.getElementById(iid).value = v;
}

function submitRoute() {
  var provider = document.getElementById('modal-provider').value;
  var model = document.getElementById('modal-claude-input').value.trim();
  var targetModel = document.getElementById('modal-target-input').value.trim();

  // If "+ New Provider" selected, create provider first
  if (provider === '__new__') {
    var newName = document.getElementById('modal-new-name').value.trim();
    var newUrl = document.getElementById('modal-new-url').value.trim();
    var newToken = document.getElementById('modal-new-token').value.trim();
    var newProto = document.getElementById('modal-new-protocol').value;
    var newAuth = document.getElementById('modal-new-auth').value;
    if (!newName || !newUrl || !newToken) {
      toast('Provider name, Base URL, and Auth Token are required', 'error');
      return;
    }
    if (!model || !targetModel) {
      toast('Claude Model and Provider Model are required', 'error');
      return;
    }
    // Create provider then route
    fetch(API + '/api/providers', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ name: newName, base_url: newUrl, auth_token: newToken, protocol: newProto, auth_type: newAuth })
    }).then(function(r) {
      if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
      // Now create route
      return fetch(API + '/api/routes', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ model: model, provider: newName, target_model: targetModel, priority: 1 })
      });
    }).then(function(r) {
      if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
      toast('Provider "' + newName + '" created, route ' + model + ' → ' + targetModel + ' added');
      closeModal();
      loadRoutes();
    }).catch(function(e) { toast('Error: ' + e.message, 'error'); });
    return;
  }

  if (!provider || !model || !targetModel) {
    toast('All fields are required', 'error');
    return;
  }
  fetch(API + '/api/routes', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ model: model, provider: provider, target_model: targetModel, priority: 1 })
  }).then(function(r) {
    if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
    toast('Route ' + model + ' → ' + targetModel + ' created');
    closeModal();
    loadRoutes();
  }).catch(function(e) { toast('Error: ' + e.message, 'error'); });
}

document.addEventListener('keydown', function(e) {
  if (e.key === 'Escape') {
    closeModal();
    closeEditProvider();
    closeEditRoute();
  }
});

// --- Usage Stats ---
function loadStats() {
  var today = new Date().toISOString().slice(0, 10);
  fetch(API + '/api/usage?date=' + today).then(function(r) { return r.json(); }).then(function(data) {
    document.getElementById('stat-requests').textContent = (data.total_requests || 0).toLocaleString();
    document.getElementById('stat-input').textContent = (data.total_input_tokens || 0).toLocaleString();
    document.getElementById('stat-output').textContent = (data.total_output_tokens || 0).toLocaleString();
    document.getElementById('stat-cost').textContent = '$' + (data.total_cost_usd || 0).toFixed(4);
  });
}

// --- Charts ---
var LINE_COLORS = ['#818cf8','#34d399','#fbbf24','#f472b6','#38bdf8','#fb923c','#a78bfa','#f87171','#2dd4bf','#e879f9'];
var rawData = [];
var barChart = null;
var tlChart = null;
var drillStack = [];
var chartsBuilt = false;

function fmt(n) {
  if (n >= 1e6) return (n/1e6).toFixed(1) + 'M';
  if (n >= 1e3) return (n/1e3).toFixed(1) + 'K';
  return String(n);
}
function getToken(r) { return (r.input_tokens || 0) + (r.output_tokens || 0); }

function fetchUsage(cb) {
  fetch(API + '/api/usage?date=' + new Date().toISOString().slice(0, 10))
    .then(function(r) { return r.json(); })
    .then(function(d) { rawData = d.records || []; cb(); })
    .catch(function() { cb(); });
}

function buildBar() {
  var byModel = {};
  rawData.forEach(function(r) {
    var k = r.model || 'unknown';
    byModel[k] = (byModel[k] || 0) + getToken(r);
  });
  var items = Object.keys(byModel).map(function(k) { return { name: k, val: byModel[k] }; });
  items.sort(function(a, b) { return b.val - a.val; });
  if (!items.length) return;

  var total = items.reduce(function(s, i) { return s + i.val; }, 0);
  var h = Math.max(100, items.length * 44 + 20);
  document.getElementById('barWrap').style.height = h + 'px';

  var ctx = document.getElementById('barChart').getContext('2d');
  barChart = new Chart(ctx, {
    type: 'bar',
    data: {
      labels: items.map(function(i) {
        var pct = total > 0 ? ((i.val / total) * 100).toFixed(0) : '0';
        return i.name + '   ' + pct + '%';
      }),
      datasets: [{
        data: items.map(function(i) { return i.val; }),
        backgroundColor: items.map(function(_, i) { return LINE_COLORS[i % LINE_COLORS.length] + '55'; }),
        borderColor: items.map(function(_, i) { return LINE_COLORS[i % LINE_COLORS.length]; }),
        borderWidth: 1, borderRadius: 4, barThickness: 22
      }]
    },
    options: {
      indexAxis: 'y', responsive: true, maintainAspectRatio: false,
      plugins: {
        legend: { display: false },
        tooltip: {
          backgroundColor: '#18181b', titleColor: '#fafafa', bodyColor: '#fafafa',
          borderColor: '#27272a', borderWidth: 1, padding: 10,
          callbacks: { label: function(c) { return fmt(c.raw) + ' tokens'; } }
        }
      },
      scales: {
        x: { ticks: { color: '#52525b', font: { size: 11 }, callback: function(v) { return fmt(v); } }, grid: { color: 'rgba(255,255,255,0.03)' }, border: { display: false } },
        y: { ticks: { color: '#a1a1aa', font: { size: 12, weight: '500' } }, grid: { display: false }, border: { display: false } }
      }
    }
  });
}

function bucketKey(ts, gran) {
  if (gran === 'hour') return ts.slice(0, 13);
  if (gran === 'week') { var dt = new Date(ts); dt.setDate(dt.getDate() - dt.getDay()); return dt.toISOString().slice(0, 10); }
  if (gran === 'month') return ts.slice(0, 7);
  return ts.slice(0, 10);
}
function granLabel(key, gran) {
  if (gran === 'hour') return key.replace('T', ' ');
  if (gran === 'month') return key;
  return key.slice(5);
}
function getGran() { return document.getElementById('gran-select').value; }

function buildTimeline() {
  if (tlChart) { tlChart.destroy(); tlChart = null; }
  var gran = getGran();
  var providerFilter = drillStack.length ? drillStack[drillStack.length - 1] : null;
  var bySeries = {};
  var allBuckets = {};
  rawData.forEach(function(r) {
    var prov = r.provider || 'unknown';
    var mod = r.model || 'unknown';
    if (providerFilter && prov !== providerFilter) return;
    var seriesName = providerFilter ? mod : prov;
    var bk = bucketKey(r.ts, gran);
    allBuckets[bk] = 1;
    if (!bySeries[seriesName]) bySeries[seriesName] = {};
    bySeries[seriesName][bk] = (bySeries[seriesName][bk] || 0) + getToken(r);
  });
  var buckets = Object.keys(allBuckets).sort();
  if (!buckets.length) return;
  var seriesNames = Object.keys(bySeries).sort();
  var datasets = seriesNames.map(function(name, i) {
    return {
      label: name,
      data: buckets.map(function(b) { return bySeries[name][b] || 0; }),
      borderColor: LINE_COLORS[i % LINE_COLORS.length],
      backgroundColor: LINE_COLORS[i % LINE_COLORS.length] + '18',
      borderWidth: 2, pointRadius: 3,
      pointBackgroundColor: LINE_COLORS[i % LINE_COLORS.length],
      pointBorderWidth: 0, tension: 0.35, fill: false
    };
  });
  var title = 'Usage Timeline';
  if (providerFilter) title = providerFilter + ' — Models';
  document.getElementById('timeline-title').textContent = title;
  document.getElementById('timeline-back').style.display = drillStack.length ? '' : 'none';
  var ctx = document.getElementById('timelineChart').getContext('2d');
  tlChart = new Chart(ctx, {
    type: 'line',
    data: { labels: buckets.map(function(b) { return granLabel(b, gran); }), datasets: datasets },
    options: {
      responsive: true,
      interaction: { mode: 'index', intersect: false },
      scales: {
        x: { ticks: { color: '#52525b', font: { size: 11 }, maxRotation: 0, maxTicksLimit: 12 }, grid: { color: 'rgba(255,255,255,0.03)' }, border: { display: false } },
        y: { ticks: { color: '#52525b', font: { size: 11 }, callback: function(v) { return fmt(v); } }, grid: { color: 'rgba(255,255,255,0.03)' }, border: { display: false }, beginAtZero: true }
      },
      plugins: {
        legend: { labels: { color: '#a1a1aa', usePointStyle: true, pointStyle: 'circle', padding: 14, font: { size: 12 } } },
        tooltip: {
          backgroundColor: '#18181b', titleColor: '#fafafa', bodyColor: '#fafafa',
          borderColor: '#27272a', borderWidth: 1, padding: 10,
          callbacks: { label: function(c) { return c.dataset.label + ': ' + fmt(c.raw) + ' tokens'; } }
        }
      },
      onClick: function(e, elements) {
        if (!elements.length || providerFilter) return;
        var idx = elements[0].datasetIndex;
        if (idx >= seriesNames.length) return;
        drillStack.push(seriesNames[idx]);
        buildTimeline();
      }
    }
  });
}

function buildAll() { buildBar(); buildTimeline(); }
window.onGranChange = function() { buildTimeline(); };
window.timelineGoBack = function() { if (drillStack.length) { drillStack.pop(); buildTimeline(); } };
window._buildCharts = function() { fetchUsage(buildAll); };

// --- Playground ---
function togglePgAdv() { document.getElementById('pg-adv').classList.toggle('open'); }

var pgSending = false;

function pgLoadModels() {
  // Always re-fetch on tab switch so newly added routes are visible
  fetch(API + '/api/routes').then(function(r) { return r.json(); }).then(function(data) {
    var routes = data.routes || data;
    var sel = document.getElementById('pg-model');
    sel.innerHTML = '<option value="">Select model</option>';
    var keys = Object.keys(routes).sort();
    keys.forEach(function(k) {
      var entry = routes[k];
      var t = (entry.targets && entry.targets[0]) || {};
      var label = k + '  →  ' + (t.provider || '?') + ' / ' + (t.model || '?');
      sel.innerHTML += '<option value="' + esc(k) + '">' + esc(label) + '</option>';
    });
  });
}

function pgOnKey(e) { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); pgSend(); } }

function pgSend() {
  if (pgSending) return;
  var model = document.getElementById('pg-model').value;
  var message = document.getElementById('pg-input').value.trim();
  if (!model) { toast('Select a model first', 'error'); return; }
  if (!message) return;

  pgSending = true;
  document.getElementById('pg-send-btn').disabled = true;
  document.getElementById('pg-input').value = '';

  var chat = document.getElementById('pg-chat');
  var empty = chat.querySelector('.pg-empty');
  if (empty) empty.remove();

  appendMsg('user', escapeHtml(message));

  // Find provider for this model — refresh from API
  var assEl = appendMsg('assistant', '');
  var bodyEl = assEl.querySelector('.pg-msg-bubble');
  bodyEl.classList.add('pg-typing');

  fetch(API + '/api/routes').then(function(r) { return r.json(); }).then(function(routes) {
    var routeEntry = (routes.routes || routes)[model];
    var target = routeEntry && routeEntry.targets && routeEntry.targets[0];
    var provider = target ? target.provider : '';
    var targetModel = target ? target.model : '';
    if (!provider) throw new Error('No provider found for model "' + model + '"');

    return fetch(API + '/api/playground', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ provider: provider, model: model, target_model: targetModel || model, message: message })
    }).then(function(resp) {
      if (!resp.ok) return resp.json().then(function(d) { throw new Error(d.error || 'Request failed'); });
      bodyEl.classList.remove('pg-typing');
      return resp.json();
    });
  }).then(function(data) {
    var text = '';
    if (data.choices && data.choices[0] && data.choices[0].message) {
      text = data.choices[0].message.content || '';
    } else if (data.content) {
      text = Array.isArray(data.content) ? data.content.map(function(b) { return b.text || ''; }).join('') : data.content;
    } else {
      text = JSON.stringify(data, null, 2);
    }
    bodyEl.textContent = text;
    finish();
  }).catch(function(err) {
    bodyEl.classList.remove('pg-typing');
    bodyEl.textContent = 'Error: ' + err.message;
    assEl.querySelector('.pg-msg-role').className = 'pg-msg-role error';
    finish();
  });

  function finish() {
    pgSending = false;
    document.getElementById('pg-send-btn').disabled = false;
    chat.scrollTop = chat.scrollHeight;
  }
}

function appendMsg(role, html) {
  var chat = document.getElementById('pg-chat');
  var div = document.createElement('div');
  var cls = role === 'user' ? 'user-msg' : 'assistant-msg';
  div.className = 'pg-msg ' + cls;
  var label = role === 'user' ? 'You' : 'Assistant';
  div.innerHTML = '<div class="pg-msg-role ' + role + '">' + label + '</div><div class="pg-msg-bubble">' + html + '</div>';
  chat.appendChild(div);
  chat.scrollTop = chat.scrollHeight;
  return div;
}

function escapeHtml(s) {
  var d = document.createElement('div');
  d.textContent = s;
  return d.innerHTML;
}

function esc(s) { return escapeHtml(s); }
function escJs(s) { return s.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/"/g, '\\"'); }

// --- Init ---
loadStats();
loadTemplates();
loadRoutes();

// Restore tab from URL hash after all functions are defined
(function() {
  var hash = location.hash.replace('#', '');
  if (hash && document.getElementById('panel-' + hash)) {
    switchTab(hash);
  }
})();
