// ecc Dashboard v2 — Provider-centric management, usage stats, playground

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
  if (tabName === 'sessions') loadSessions();
  if (tabName === 'playground') pgLoadModels();
}

document.querySelectorAll('.tab').forEach(function(btn) {
  btn.addEventListener('click', function() { switchTab(btn.dataset.tab); });
});

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

function esc(s) { if (!s) return ''; var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }
function escJs(s) { return s.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/"/g, '\\"'); }

// --- Preset data ---
var presetData = [];

function loadPresets() {
  fetch(API + '/api/presets').then(function(r) { return r.json(); }).then(function(data) {
    presetData = Array.isArray(data) ? data : [];
    populatePresetSelect();
  }).catch(function() { presetData = []; });
}

function populatePresetSelect() {
  var sel = document.getElementById('modal-preset');
  if (!sel) return;
  sel.innerHTML = '<option value="">Choose a provider preset...</option>';
  presetData.forEach(function(p) {
    sel.innerHTML += '<option value="' + esc(p.name) + '">' + esc(p.name) + '</option>';
  });
}

function findPreset(name) {
  for (var i = 0; i < presetData.length; i++) {
    if (presetData[i].name === name) return presetData[i];
  }
  return null;
}

// --- Routes (Provider-centric) ---
var providers = [];
var routesMap = {};
var CARD_COLORS = ['#818cf8', '#34d399', '#fbbf24', '#f472b6', '#38bdf8', '#fb923c', '#a78bfa', '#2dd4bf'];

function loadRoutes() {
  Promise.all([
    fetch(API + '/api/providers').then(function(r) { return r.json(); }),
    fetch(API + '/api/routes').then(function(r) { return r.json(); })
  ]).then(function(results) {
    providers = Array.isArray(results[0]) ? results[0] : [];
    routesMap = results[1] || {};
    renderProviderCards();
  }).catch(function(e) { console.error('loadRoutes failed:', e); });
}

function renderProviderCards() {
  var container = document.getElementById('provider-cards');
  container.innerHTML = '';

  if (providers.length === 0) {
    container.innerHTML = '<div class="route-empty">No providers configured. Click "+ Add Provider" to create one from a preset.</div>';
    return;
  }

  providers.sort(function(a, b) { return a.name.localeCompare(b.name); });
  providers.forEach(function(prov, pi) {
    var color = CARD_COLORS[pi % CARD_COLORS.length];
    var card = document.createElement('div');
    card.className = 'provider-card';
    card.style.borderTopColor = color;

    // Header
    var head = document.createElement('div');
    head.className = 'provider-card-head';
    head.innerHTML =
      '<div class="provider-card-name">' + esc(prov.name) +
      ' <span class="tag tag-protocol">' + esc(prov.protocol) + '</span>' +
      (prov.is_coding_plan ? ' <span class="tag tag-protocol" style="background:#34d39933;color:#34d399;">Coding Plan</span>' : '') +
      '</div>' +
      '<span class="icon-bar">' +
      '<button class="icon-btn icon-btn-edit" title="Edit provider" onclick="editProvider(\'' + escJs(prov.name) + '\')">' +
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>' +
      '</button>' +
      '<button class="icon-btn" title="Delete provider" onclick="deleteProvider(\'' + escJs(prov.name) + '\')">' +
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>' +
      '</button></span>';

    // Body — show model mappings
    var body = document.createElement('div');
    body.className = 'provider-card-body';

    var mappings = prov.model_mappings || [];
    if (mappings.length === 0) {
      body.innerHTML = '<div style="padding:8px;color:var(--text-3);font-size:12px;">No model mappings configured</div>';
    } else {
      mappings.sort(function(a, b) { return a.claude_model.localeCompare(b.claude_model); });
      mappings.forEach(function(m) {
        var item = document.createElement('div');
        item.className = 'route-item';
        item.innerHTML =
          '<div class="route-models">' +
          '<span>' + esc(m.claude_model) + '</span>' +
          '<span class="route-arrow">&rarr;</span>' +
          '<span class="route-target">' + esc(m.provider_model) + '</span>' +
          '</div>' +
          '<span class="icon-bar">' +
          '<button class="icon-btn icon-btn-chart" title="Usage detail" onclick="showModelDetail(\'' + escJs(prov.name) + '\',\'' + escJs(m.provider_model) + '\')">' +
          '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="20" x2="18" y2="10"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="6" y1="20" x2="6" y2="14"/></svg>' +
          '</button>' +
          '<button class="icon-btn" title="Delete mapping" onclick="deleteMapping(\'' + escJs(prov.name) + '\',\'' + escJs(m.claude_model) + '\')">' +
          '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>' +
          '</button></span>';
        body.appendChild(item);
      });
    }
    // Add mapping button
    var addBtn = document.createElement('button');
    addBtn.className = 'btn' ;
    addBtn.style.cssText = 'font-size:12px;padding:4px 10px;margin:8px 0 4px;background:var(--surface-2);color:var(--text-2);';
    addBtn.textContent = '+ Add Mapping';
    addBtn.onclick = function() { openAddMappingModal(prov.name); };
    body.appendChild(addBtn);

    // Quota / cost info section
    var infoDiv = document.createElement('div');
    infoDiv.className = 'quota-section';
    infoDiv.id = 'info-' + escJs(prov.name);

    card.appendChild(head);
    card.appendChild(body);
    card.appendChild(infoDiv);
    container.appendChild(card);
  });

  // Load quota info for all providers
  loadProviderInfo();
}

// --- Provider Info (quota / cost) ---
var quotaCache = {};

function loadProviderInfo() {
  fetch(API + '/api/quota').then(function(r) { return r.json(); }).then(function(data) {
    quotaCache = data;
    providers.forEach(function(prov) {
      var el = document.getElementById('info-' + prov.name);
      if (!el) return;
      var info = data[prov.name];
      if (prov.is_coding_plan && info) {
        renderQuota(el, info);
      }
    });
  }).catch(function() {});
}

function renderQuota(el, data) {
  if (!data.success) { el.style.display = 'none'; return; }
  var tiers = data.tiers || [];
  if (!tiers.length) { el.style.display = 'none'; return; }
  var tierThemes = {
    five_hour: { ring: '#22d3ee', glow: 'rgba(34,211,238,0.35)', label: '5h' },
    weekly_limit: { ring: '#818cf8', glow: 'rgba(129,140,248,0.35)', label: 'Weekly' },
    mcp_monthly: { ring: '#34d399', glow: 'rgba(52,211,153,0.35)', label: 'MCP/Mo' }
  };
  var html = '';
  tiers.forEach(function(t) {
    var pct = Math.round(t.utilization);
    var theme = tierThemes[t.name] || { ring: '#818cf8', glow: 'rgba(129,140,248,0.35)', label: t.name };
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

// --- Add/Delete Mapping ---
function openAddMappingModal(providerName) {
  document.getElementById('am-provider').textContent = providerName;
  document.getElementById('am-claude').value = '';
  document.getElementById('am-target').value = '';
  document.getElementById('add-mapping-modal').style.display = 'flex';
}

function closeAddMapping() {
  document.getElementById('add-mapping-modal').style.display = 'none';
}

function submitAddMapping() {
  var providerName = document.getElementById('am-provider').textContent;
  var claudeModel = document.getElementById('am-claude').value.trim();
  var targetModel = document.getElementById('am-target').value.trim();
  if (!claudeModel || !targetModel) {
    toast('Both Claude Model and Provider Model are required', 'error');
    return;
  }
  var prov = providers.find(function(p) { return p.name === providerName; });
  if (!prov) return;
  var mappings = (prov.model_mappings || []).slice();
  // Check duplicate
  var dup = mappings.find(function(m) { return m.claude_model === claudeModel; });
  if (dup) {
    toast('Mapping for ' + claudeModel + ' already exists', 'error');
    return;
  }
  mappings.push({ claude_model: claudeModel, provider_model: targetModel });
  fetch(API + '/api/providers/' + encodeURIComponent(providerName), {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ model_mappings: mappings })
  }).then(function(r) {
    if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
    toast('Mapping added: ' + claudeModel + ' → ' + targetModel);
    closeAddMapping();
    loadRoutes();
  }).catch(function(e) { toast('Error: ' + e.message, 'error'); });
}

function deleteMapping(providerName, claudeModel) {
  if (!confirm('Delete mapping "' + claudeModel + '" from ' + providerName + '?')) return;
  var prov = providers.find(function(p) { return p.name === providerName; });
  if (!prov) return;
  var mappings = (prov.model_mappings || []).filter(function(m) { return m.claude_model !== claudeModel; });
  fetch(API + '/api/providers/' + encodeURIComponent(providerName), {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ model_mappings: mappings })
  }).then(function(r) {
    if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
    toast('Mapping deleted');
    loadRoutes();
  }).catch(function(e) { toast('Error: ' + e.message, 'error'); });
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
  var p = providers.find(function(x) { return x.name === name; });
  if (!p) return;
  document.getElementById('ep-name').value = name;
  document.getElementById('ep-url').value = p.base_url || '';
  document.getElementById('ep-token').value = '';
  document.getElementById('ep-token').placeholder = p.auth_token ? '(current token hidden)' : 'sk-...';
  document.getElementById('ep-protocol').value = p.protocol || 'anthropic';
  document.getElementById('ep-auth').value = p.auth_type || 'bearer';
  document.getElementById('ep-coding-plan').value = String(!!p.is_coding_plan);
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
  if (url) data.base_url = url;
  if (token) data.auth_token = token;
  data.protocol = document.getElementById('ep-protocol').value;
  data.auth_type = document.getElementById('ep-auth').value;
  data.is_coding_plan = document.getElementById('ep-coding-plan').value === 'true';
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

// --- Add Provider Modal (Preset-driven) ---
function openRouteModal() {
  document.getElementById('modal-new-name').value = '';
  document.getElementById('modal-new-url').value = '';
  document.getElementById('modal-new-token').value = '';
  document.getElementById('modal-new-protocol').value = 'openai';
  document.getElementById('modal-new-auth').value = 'bearer';
  document.getElementById('modal-new-coding-plan').value = 'false';
  document.getElementById('modal-preset').value = '';
  populatePresetSelect();
  document.getElementById('route-modal').style.display = 'flex';
}

function closeModal() {
  document.getElementById('route-modal').style.display = 'none';
}

function onPresetChange() {
  var name = document.getElementById('modal-preset').value;
  if (!name) return;
  var preset = findPreset(name);
  if (!preset) return;
  document.getElementById('modal-new-name').value = preset.name.toLowerCase().replace(/\s+/g, '-');
  document.getElementById('modal-new-protocol').value = preset.protocol || 'anthropic';
  document.getElementById('modal-new-auth').value = preset.auth_type || 'bearer';
  // Use alt_base_urls if available
  var alt = preset.alt_base_urls || {};
  var proto = preset.protocol || 'anthropic';
  var url = alt[proto] || preset.base_url;
  document.getElementById('modal-new-url').value = url;
}

function submitNewProvider() {
  var name = document.getElementById('modal-new-name').value.trim();
  var url = document.getElementById('modal-new-url').value.trim();
  var token = document.getElementById('modal-new-token').value.trim();
  var proto = document.getElementById('modal-new-protocol').value;
  var auth = document.getElementById('modal-new-auth').value;
  var isCodingPlan = document.getElementById('modal-new-coding-plan').value === 'true';

  if (!name || !url || !token) {
    toast('Name, Base URL, and Auth Token are required', 'error');
    return;
  }

  // Find matching preset to get suggested_mappings and pricing
  var preset = findPreset(document.getElementById('modal-preset').value);
  var mappings = [];
  var pricing = {};
  if (preset) {
    mappings = (preset.suggested_mappings || []).map(function(m) {
      return { claude_model: m.claude_model, provider_model: m.provider_model };
    });
    pricing = preset.pricing || {};
    // Copy quota_adapter if available
  }

  var body = {
    name: name,
    base_url: url,
    auth_token: token,
    protocol: proto,
    auth_type: auth,
    is_coding_plan: isCodingPlan,
    model_mappings: mappings,
    pricing: pricing
  };

  if (preset && preset.quota_adapter) {
    body.quota_adapter = preset.quota_adapter;
  }

  fetch(API + '/api/providers', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body)
  }).then(function(r) {
    if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
    toast('Provider "' + name + '" created' + (mappings.length ? ' with ' + mappings.length + ' route(s)' : ''));
    closeModal();
    loadRoutes();
  }).catch(function(e) { toast('Error: ' + e.message, 'error'); });
}

document.addEventListener('keydown', function(e) {
  if (e.key === 'Escape') {
    closeModal();
    closeEditProvider();
    closeAddMapping();
    closeDetail();
    closeSessionDetail();
  }
});

// --- Usage Stats ---
function loadStats() {
  fetch(API + '/api/usage').then(function(r) { return r.json(); }).then(function(data) {
    var arr = Array.isArray(data) ? data : [];
    var totalReq = 0, totalIn = 0, totalOut = 0, totalCost = 0;
    arr.forEach(function(u) {
      totalReq += u.total_requests || 0;
      totalIn += u.total_input_tokens || 0;
      totalOut += u.total_output_tokens || 0;
      totalCost += u.total_cost_usd || 0;
    });
    document.getElementById('stat-requests').textContent = totalReq.toLocaleString();
    document.getElementById('stat-input').textContent = fmt(totalIn);
    document.getElementById('stat-output').textContent = fmt(totalOut);
    document.getElementById('stat-cost').textContent = '$' + totalCost.toFixed(4);
  }).catch(function() {});
}

// --- Charts ---
var LINE_COLORS = ['#818cf8','#34d399','#fbbf24','#f472b6','#38bdf8','#fb923c','#a78bfa','#f87171','#2dd4bf','#e879f9'];
var barChart = null;

function fmt(n) {
  if (n >= 1e6) return (n/1e6).toFixed(1) + 'M';
  if (n >= 1e3) return (n/1e3).toFixed(1) + 'K';
  return String(n);
}

function fetchUsage(cb) {
  fetch(API + '/api/usage')
    .then(function(r) { return r.json(); })
    .then(function(d) { cb(Array.isArray(d) ? d : []); })
    .catch(function() { cb([]); });
}

function buildBar(usageData) {
  if (barChart) { barChart.destroy(); barChart = null; }
  if (!usageData.length) return;
  var items = usageData.map(function(u) {
    return { name: u.provider_name, tokens: (u.total_input_tokens || 0) + (u.total_output_tokens || 0) };
  });
  items.sort(function(a, b) { return b.tokens - a.tokens; });
  var total = items.reduce(function(s, i) { return s + i.tokens; }, 0);

  var ctx = document.getElementById('barChart').getContext('2d');
  barChart = new Chart(ctx, {
    type: 'bar',
    data: {
      labels: items.map(function(i) {
        var pct = total > 0 ? ((i.tokens / total) * 100).toFixed(0) : '0';
        return i.name + '   ' + pct + '%';
      }),
      datasets: [{
        data: items.map(function(i) { return i.tokens; }),
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

// --- Timeline ---
var timelineChart = null;
var timelineGran = 'day';
var timelineDrillProvider = null;

function onGranChange() {
  timelineGran = document.getElementById('gran-select').value;
  buildTimeline();
}

function timelineGoBack() {
  timelineDrillProvider = null;
  document.getElementById('timeline-back').style.display = 'none';
  document.getElementById('timeline-title').textContent = 'Usage Timeline';
  buildTimeline();
}

function generateBuckets(gran) {
  var now = new Date();
  var keys = [], labels = [];
  var i, d, Y, M, D, h, prevMonth;

  if (gran === 'hour') {
    for (i = 23; i >= 0; i--) {
      d = new Date(now.getTime() - i * 3600000);
      Y = d.getFullYear(); M = pad2(d.getMonth() + 1); D = pad2(d.getDate()); h = pad2(d.getHours());
      keys.push(Y + '-' + M + '-' + D + ' ' + h + ':00');
      labels.push(h + ':00');
    }
  } else if (gran === 'day') {
    for (i = 29; i >= 0; i--) {
      d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i);
      Y = d.getFullYear(); M = pad2(d.getMonth() + 1); D = pad2(d.getDate());
      keys.push(Y + '-' + M + '-' + D);
      labels.push(M + '-' + D);
    }
  } else if (gran === 'week') {
    var dow = now.getDay();
    var mon = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (dow === 0 ? 6 : dow - 1));
    for (i = 11; i >= 0; i--) {
      d = new Date(mon.getTime() - i * 7 * 86400000);
      Y = d.getFullYear(); M = pad2(d.getMonth() + 1); D = pad2(d.getDate());
      keys.push(Y + '-W' + M + D);
      labels.push(M + '-' + D);
    }
  } else {
    for (i = 11; i >= 0; i--) {
      d = new Date(now.getFullYear(), now.getMonth() - i, 1);
      Y = d.getFullYear(); M = pad2(d.getMonth() + 1);
      keys.push(Y + '-' + M);
      labels.push(Y + '-' + M);
    }
  }

  return { keys: keys, labels: labels };
}

function pad2(n) { return ('0' + n).slice(-2); }

function recordBucketKey(ts, gran) {
  var d = new Date(ts);
  var Y = d.getFullYear(), M = pad2(d.getMonth() + 1), D = pad2(d.getDate()), h = pad2(d.getHours());
  if (gran === 'hour') return Y + '-' + M + '-' + D + ' ' + h + ':00';
  if (gran === 'week') return Y + '-W' + pad2(d.getMonth() + 1) + pad2(d.getDate());
  if (gran === 'month') return Y + '-' + M;
  return Y + '-' + M + '-' + D;
}

function buildTimeline() {
  if (timelineChart) { timelineChart.destroy(); timelineChart = null; }

  var fetchDays = { hour: 1, day: 30, week: 90, month: 365 }[timelineGran] || 30;
  var url = API + '/api/usage/detail?days=' + fetchDays;
  if (timelineDrillProvider) {
    url += '&provider=' + encodeURIComponent(timelineDrillProvider);
  }

  fetch(url)
    .then(function(r) { return r.json(); })
    .then(function(records) {
      var arr = Array.isArray(records) ? records : [];
      var b = generateBuckets(timelineGran);
      var groupField = timelineDrillProvider ? 'target_model' : 'provider_name';

      var agg = {};
      var groupSet = {};
      arr.forEach(function(r) {
        var gval = r[groupField] || 'unknown';
        var bk = recordBucketKey(r.timestamp, timelineGran);
        agg[gval + '||' + bk] = (agg[gval + '||' + bk] || 0) + (r.input_tokens || 0) + (r.output_tokens || 0);
        groupSet[gval] = true;
      });

      var groupNames = Object.keys(groupSet).sort();

      var datasets = groupNames.map(function(name, i) {
        var color = LINE_COLORS[i % LINE_COLORS.length];
        return {
          label: name,
          data: b.keys.map(function(bk) { return agg[name + '||' + bk] || 0; }),
          borderColor: color,
          backgroundColor: color + '18',
          fill: false, tension: 0.1, pointRadius: 2, pointHoverRadius: 5, borderWidth: 2,
          spanGaps: false
        };
      });

      var ctx = document.getElementById('timelineChart').getContext('2d');
      timelineChart = new Chart(ctx, {
        type: 'line',
        data: { labels: b.labels, datasets: datasets },
        options: {
          responsive: true, maintainAspectRatio: false,
          interaction: { mode: 'index', intersect: false },
          onClick: function(evt, elements) {
            if (timelineDrillProvider) return;
            if (!elements.length) return;
            timelineDrillProvider = groupNames[elements[0].datasetIndex];
            document.getElementById('timeline-title').textContent = timelineDrillProvider + ' — Models';
            document.getElementById('timeline-back').style.display = 'inline-flex';
            buildTimeline();
          },
          plugins: {
            legend: { labels: { color: '#a1a1aa', boxWidth: 12, padding: 16 } },
            tooltip: {
              backgroundColor: '#18181b', titleColor: '#fafafa', bodyColor: '#fafafa',
              borderColor: '#27272a', borderWidth: 1, padding: 10,
              callbacks: { label: function(c) { return c.dataset.label + ': ' + fmt(c.raw) + ' tokens'; } }
            }
          },
          scales: {
            x: { ticks: { color: '#52525b', font: { size: 11 }, maxRotation: 45 }, grid: { color: 'rgba(255,255,255,0.03)' }, border: { display: false } },
            y: { beginAtZero: true, ticks: { color: '#52525b', font: { size: 11 }, callback: function(v) { return fmt(v); } }, grid: { color: 'rgba(255,255,255,0.03)' }, border: { display: false } }
          }
        }
      });
    })
    .catch(function() {});
}

window._buildCharts = function() {
  fetchUsage(function(data) { buildBar(data); });
  buildTimeline();
};

// --- Model Detail ---
var detailChart = null;
var detailRecords = [];
var detailPage = 0;
var detailPageSize = 10;
var detailProvider = '';
var detailModel = '';

function showModelDetail(provider, model) {
  detailProvider = provider;
  detailModel = model;
  document.getElementById('detail-title').textContent = provider + ' / ' + model;
  document.getElementById('detail-modal').style.display = 'flex';

  // Default: last 7 days
  var now = new Date();
  var from = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 6);
  document.getElementById('detail-from').value = from.toISOString().slice(0, 10);
  document.getElementById('detail-to').value = now.toISOString().slice(0, 10);

  loadDetailData();
}

function detailFilterByRange() {
  loadDetailData();
}

function loadDetailData() {
  document.getElementById('detail-summary').innerHTML = '<div class="detail-loading">Loading...</div>';
  document.getElementById('detail-tbody').innerHTML = '';
  document.getElementById('detail-pager').style.display = 'none';
  if (detailChart) { detailChart.destroy(); detailChart = null; }

  var fromVal = document.getElementById('detail-from').value;
  var toVal = document.getElementById('detail-to').value;

  // Calculate days for API query (generous to cover full date range)
  var days = 30;
  if (fromVal) {
    var diff = Date.now() - new Date(fromVal).getTime();
    days = Math.max(Math.ceil(diff / 86400000) + 1, 1);
  }

  fetch(API + '/api/usage/detail?provider=' + encodeURIComponent(detailProvider) + '&model=' + encodeURIComponent(detailModel) + '&days=' + days)
    .then(function(r) { return r.json(); })
    .then(function(records) {
      var arr = Array.isArray(records) ? records : [];

      // Client-side date filter
      if (fromVal) {
        var fromMs = new Date(fromVal).getTime();
        arr = arr.filter(function(r) { return new Date(r.timestamp).getTime() >= fromMs; });
      }
      if (toVal) {
        var toMs = new Date(toVal).getTime() + 86400000; // include the whole day
        arr = arr.filter(function(r) { return new Date(r.timestamp).getTime() < toMs; });
      }

      detailRecords = arr;
      detailPage = 0;

      // Summary
      var totalReq = 0, totalIn = 0, totalOut = 0, totalCost = 0;
      arr.forEach(function(r) {
        totalReq++;
        totalIn += r.input_tokens || 0;
        totalOut += r.output_tokens || 0;
        totalCost += r.cost_usd || 0;
      });
      document.getElementById('detail-summary').innerHTML =
        '<div class="detail-stat"><div class="detail-stat-val">' + totalReq + '</div><div class="detail-stat-lbl">Requests</div></div>' +
        '<div class="detail-stat"><div class="detail-stat-val">' + fmt(totalIn) + '</div><div class="detail-stat-lbl">Input</div></div>' +
        '<div class="detail-stat"><div class="detail-stat-val">' + fmt(totalOut) + '</div><div class="detail-stat-lbl">Output</div></div>' +
        '<div class="detail-stat"><div class="detail-stat-val">$' + totalCost.toFixed(4) + '</div><div class="detail-stat-lbl">Cost</div></div>';

      // Request Trend — build day buckets from selected range
      buildDetailTrend(arr, fromVal, toVal);
      renderDetailPage();
    })
    .catch(function(e) {
      document.getElementById('detail-summary').innerHTML = '<div class="detail-loading">Error: ' + esc(e.message) + '</div>';
    });
}

function buildDetailTrend(arr, fromVal, toVal) {
  if (detailChart) { detailChart.destroy(); detailChart = null; }

  var start = fromVal ? new Date(fromVal) : new Date(Date.now() - 6 * 86400000);
  var end = toVal ? new Date(toVal) : new Date();
  start = new Date(start.getFullYear(), start.getMonth(), start.getDate());
  end = new Date(end.getFullYear(), end.getMonth(), end.getDate());

  var dayKeys = [], dayLabels = [];
  var d = new Date(start);
  while (d <= end) {
    dayKeys.push(d.getFullYear() + '-' + pad2(d.getMonth() + 1) + '-' + pad2(d.getDate()));
    dayLabels.push(pad2(d.getMonth() + 1) + '-' + pad2(d.getDate()));
    d.setDate(d.getDate() + 1);
  }

  var dayAgg = {};
  arr.forEach(function(r) {
    var dd = new Date(r.timestamp);
    var k = dd.getFullYear() + '-' + pad2(dd.getMonth() + 1) + '-' + pad2(dd.getDate());
    dayAgg[k] = (dayAgg[k] || 0) + (r.input_tokens || 0) + (r.output_tokens || 0);
  });
  var trendData = dayKeys.map(function(k) { return dayAgg[k] || 0; });
  var trendMax = Math.max.apply(null, trendData);
  var yMax = Math.max(trendMax * 1.2, 1);

  var ctx = document.getElementById('detailChart').getContext('2d');
  detailChart = new Chart(ctx, {
    type: 'bar',
    data: {
      labels: dayLabels,
      datasets: [{
        label: 'Tokens',
        data: trendData,
        backgroundColor: '#818cf855',
        borderColor: '#818cf8',
        borderWidth: 1, borderRadius: 4, barThickness: 16
      }]
    },
    options: {
      responsive: true, maintainAspectRatio: false,
      plugins: {
        legend: { display: false },
        tooltip: {
          backgroundColor: '#18181b', titleColor: '#fafafa', bodyColor: '#fafafa',
          borderColor: '#27272a', borderWidth: 1, padding: 10,
          callbacks: { label: function(c) { return fmt(c.raw) + ' tokens'; } }
        }
      },
      scales: {
        x: { ticks: { color: '#52525b', font: { size: 11 }, maxRotation: 45 }, grid: { display: false }, border: { display: false } },
        y: { min: 0, max: yMax, ticks: { color: '#52525b', font: { size: 11 }, callback: function(v) { return fmt(v); } }, grid: { color: 'rgba(255,255,255,0.03)' }, border: { display: false } }
      }
    }
  });
}

function renderDetailPage() {
  var arr = detailRecords;
  var tbody = document.getElementById('detail-tbody');
  var pagerEl = document.getElementById('detail-pager');
  var totalPages = Math.ceil(arr.length / detailPageSize) || 1;

  var start = detailPage * detailPageSize;
  var slice = arr.slice(start, start + detailPageSize);

  tbody.innerHTML = '';
  if (!arr.length) {
    tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;color:var(--text-3);padding:24px;">No requests found</td></tr>';
    pagerEl.style.display = 'none';
    return;
  }

  slice.forEach(function(r) {
    var ts = r.timestamp ? r.timestamp.replace('T', ' ').slice(0, 19) : '';
    var statusClass = (r.status >= 400) ? 'detail-status-err' : 'detail-status-ok';
    tbody.innerHTML +=
      '<tr>' +
      '<td>' + esc(ts) + '</td>' +
      '<td>' + esc(r.requested_model || '') + '</td>' +
      '<td>' + fmt(r.input_tokens || 0) + '</td>' +
      '<td>' + fmt(r.output_tokens || 0) + '</td>' +
      '<td>' + (r.latency_ms || 0) + 'ms</td>' +
      '<td><span class="' + statusClass + '">' + (r.status || 0) + '</span></td>' +
      '</tr>';
  });

  pagerEl.style.display = 'flex';
  document.getElementById('detail-page-info').textContent = (detailPage + 1) + ' / ' + totalPages;
  document.getElementById('detail-prev-btn').disabled = detailPage === 0;
  document.getElementById('detail-next-btn').disabled = detailPage >= totalPages - 1;
}

function detailPrevPage() {
  if (detailPage > 0) { detailPage--; renderDetailPage(); }
}

function detailNextPage() {
  var totalPages = Math.ceil(detailRecords.length / detailPageSize);
  if (detailPage < totalPages - 1) { detailPage++; renderDetailPage(); }
}

function closeDetail() {
  document.getElementById('detail-modal').style.display = 'none';
}

// --- Sessions ---
var sessionListData = [];
var currentSessionId = '';

function loadSessions() {
  var container = document.getElementById('sessions-list');
  container.innerHTML = '<div class="detail-loading">Loading...</div>';

  fetch(API + '/api/sessions')
    .then(function(r) { return r.json(); })
    .then(function(data) {
      sessionListData = Array.isArray(data) ? data : [];
      renderSessions();
    })
    .catch(function(e) {
      container.innerHTML = '<div class="detail-loading">Error: ' + esc(e.message) + '</div>';
    });
}

function renderSessions() {
  var container = document.getElementById('sessions-list');
  if (!sessionListData.length) {
    container.innerHTML = '<div class="empty">No sessions recorded yet. Send requests through the proxy to see conversations here.</div>';
    return;
  }

  var html = '<div class="ses-toolbar">' +
    '<label class="ses-check-all"><input type="checkbox" id="ses-select-all" onchange="toggleAllSessions(this.checked)" /><span>Select All</span></label>' +
    '<button class="btn" id="ses-batch-del" onclick="batchDeleteSessions()" style="display:none;background:var(--red-bg);color:var(--red);font-size:12px;padding:4px 12px;">Delete Selected (<span id="ses-selected-count">0</span>)</button>' +
    '</div>';

  html += '<table class="ses-table"><thead><tr>' +
    '<th style="width:30px;"></th>' +
    '<th>Session</th><th>Provider</th><th>Model</th><th>Turns</th><th>Last Active</th><th></th>' +
    '</tr></thead><tbody>';

  sessionListData.forEach(function(s) {
    var shortId = s.session_id.length > 12 ? s.session_id.slice(0, 12) + '...' : s.session_id;
    var lastActive = s.last_timestamp ? timeAgo(s.last_timestamp) : '';
    html += '<tr class="ses-row" onclick="showSessionDetail(\'' + escJs(s.session_id) + '\')">' +
      '<td onclick="event.stopPropagation()"><input type="checkbox" class="ses-check" data-id="' + esc(s.session_id) + '" onchange="onSessionCheckChange()" /></td>' +
      '<td><code class="ses-id">' + esc(shortId) + '</code></td>' +
      '<td>' + esc(s.provider_name || '') + '</td>' +
      '<td>' + esc(s.requested_model || '') + '</td>' +
      '<td>' + (s.total_turns || 0) + '</td>' +
      '<td style="color:var(--text-3);">' + esc(lastActive) + '</td>' +
      '<td><button class="icon-btn" title="Delete session" onclick="event.stopPropagation();deleteSessionById(\'' + escJs(s.session_id) + '\')">' +
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>' +
      '</button></td>' +
      '</tr>';
  });

  html += '</tbody></table>';
  container.innerHTML = html;
}

function getCheckedSessionIds() {
  var checks = document.querySelectorAll('.ses-check:checked');
  var ids = [];
  checks.forEach(function(c) { ids.push(c.getAttribute('data-id')); });
  return ids;
}

function onSessionCheckChange() {
  var ids = getCheckedSessionIds();
  var btn = document.getElementById('ses-batch-del');
  var count = document.getElementById('ses-selected-count');
  if (ids.length > 0) {
    btn.style.display = 'inline-flex';
    count.textContent = ids.length;
  } else {
    btn.style.display = 'none';
  }
  // Update select-all state
  var allChecks = document.querySelectorAll('.ses-check');
  var allBox = document.getElementById('ses-select-all');
  allBox.checked = allChecks.length > 0 && ids.length === allChecks.length;
}

function toggleAllSessions(checked) {
  document.querySelectorAll('.ses-check').forEach(function(c) { c.checked = checked; });
  onSessionCheckChange();
}

function batchDeleteSessions() {
  var ids = getCheckedSessionIds();
  if (!ids.length) return;
  if (!confirm('Delete ' + ids.length + ' selected session(s)?')) return;

  var pending = ids.length;
  ids.forEach(function(id) {
    fetch(API + '/api/sessions/' + encodeURIComponent(id), { method: 'DELETE' })
      .then(function() {
        pending--;
        if (pending === 0) {
          toast(ids.length + ' session(s) deleted');
          loadSessions();
        }
      })
      .catch(function(e) { toast('Error: ' + e.message, 'error'); });
  });
}

function timeAgo(ts) {
  var diff = Date.now() - new Date(ts).getTime();
  var sec = Math.floor(diff / 1000);
  if (sec < 60) return sec + 's ago';
  var min = Math.floor(sec / 60);
  if (min < 60) return min + 'm ago';
  var hr = Math.floor(min / 60);
  if (hr < 24) return hr + 'h ago';
  var d = Math.floor(hr / 24);
  if (d < 30) return d + 'd ago';
  return ts.replace('T', ' ').slice(0, 10);
}

function showSessionDetail(sessionId) {
  currentSessionId = sessionId;
  document.getElementById('ses-title').textContent = 'Session ' + sessionId.slice(0, 16);
  document.getElementById('ses-chat').innerHTML = '<div class="detail-loading">Loading...</div>';
  document.getElementById('session-modal').style.display = 'flex';

  fetch(API + '/api/sessions/' + encodeURIComponent(sessionId))
    .then(function(r) { return r.json(); })
    .then(function(records) {
      var arr = Array.isArray(records) ? records : [];
      if (!arr.length) {
        document.getElementById('ses-chat').innerHTML = '<div class="empty">No records found</div>';
        return;
      }

      var meta = arr.length + ' turns · ' + (arr[0].provider_name || '') + ' / ' + (arr[0].target_model || '');
      document.getElementById('ses-meta').textContent = meta;

      var html = '';
      arr.forEach(function(rec, i) {
        var userMsg = extractUserMessage(rec.request_body);
        var assistantMsg = rec.assistant_text || '';
        var thinking = rec.thinking_text || '';
        var ts = rec.timestamp ? rec.timestamp.replace('T', ' ').slice(0, 19) : '';

        html += '<div class="ses-turn">' +
          '<div class="ses-turn-head">' +
          '<span class="ses-turn-num">#' + (i + 1) + '</span>' +
          '<span class="ses-turn-time">' + esc(ts) + '</span>' +
          '<span class="ses-turn-stats">' + fmt(rec.input_tokens || 0) + ' in / ' + fmt(rec.output_tokens || 0) + ' out · ' + (rec.latency_ms || 0) + 'ms</span>' +
          '</div>';

        // User message
        html += '<div class="ses-msg ses-msg-user">' +
          '<div class="ses-msg-role ses-role-user">User</div>' +
          '<div class="ses-msg-body">' + escapeHtml(userMsg) + '</div>' +
          '</div>';

        // Thinking (collapsible)
        if (thinking) {
          html += '<div class="ses-thinking-wrap">' +
            '<button class="ses-thinking-toggle" onclick="this.nextElementSibling.style.display=this.nextElementSibling.style.display===\'none\'?\'block\':\'none\';this.textContent=this.nextElementSibling.style.display===\'none\'?\'+ Thinking\':\'- Thinking\'">+ Thinking</button>' +
            '<div class="ses-thinking-body" style="display:none;">' + escapeHtml(thinking) + '</div>' +
            '</div>';
        }

        // Assistant message
        html += '<div class="ses-msg ses-msg-assistant">' +
          '<div class="ses-msg-role ses-role-assistant">Assistant</div>' +
          '<div class="ses-msg-body">' + escapeHtml(assistantMsg) + '</div>' +
          '</div>';

        html += '</div>';
      });

      document.getElementById('ses-chat').innerHTML = html;
      document.getElementById('ses-chat').scrollTop = document.getElementById('ses-chat').scrollHeight;
    })
    .catch(function(e) {
      document.getElementById('ses-chat').innerHTML = '<div class="detail-loading">Error: ' + esc(e.message) + '</div>';
    });
}

function extractUserMessage(body) {
  try {
    var obj = JSON.parse(body);
    var messages = obj.messages || [];
    // Get the last user message
    for (var i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === 'user') {
        var c = messages[i].content;
        if (typeof c === 'string') return c;
        if (Array.isArray(c)) {
          return c.filter(function(b) { return b.type === 'text'; }).map(function(b) { return b.text || ''; }).join('\n');
        }
        return JSON.stringify(c);
      }
    }
  } catch(e) {}
  return body.slice(0, 500);
}

function deleteSession() {
  if (!currentSessionId) return;
  if (!confirm('Delete this entire session?')) return;
  deleteSessionById(currentSessionId, function() {
    closeSessionDetail();
    loadSessions();
  });
}

function deleteSessionById(id, cb) {
  fetch(API + '/api/sessions/' + encodeURIComponent(id), { method: 'DELETE' })
    .then(function(r) {
      if (!r.ok) return r.json().then(function(d) { throw new Error(d.error); });
      toast('Session deleted');
      if (cb) cb(); else loadSessions();
    })
    .catch(function(e) { toast('Error: ' + e.message, 'error'); });
}

function closeSessionDetail() {
  document.getElementById('session-modal').style.display = 'none';
  currentSessionId = '';
}

// --- Playground ---
var pgSending = false;

function pgLoadModels() {
  // Build model list from providers' mappings
  fetch(API + '/api/providers').then(function(r) { return r.json(); }).then(function(data) {
    var provList = Array.isArray(data) ? data : [];
    var sel = document.getElementById('pg-model');
    sel.innerHTML = '<option value="">Select model</option>';
    provList.forEach(function(prov) {
      (prov.model_mappings || []).forEach(function(m) {
        var label = m.claude_model + '  →  ' + prov.name + ' / ' + m.provider_model;
        sel.innerHTML += '<option value="' + esc(m.claude_model) + '" data-provider="' + esc(prov.name) + '" data-target="' + esc(m.provider_model) + '">' + esc(label) + '</option>';
      });
    });
  });
}

function pgOnKey(e) { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); pgSend(); } }

function pgSend() {
  if (pgSending) return;
  var sel = document.getElementById('pg-model');
  var model = sel.value;
  var opt = sel.options[sel.selectedIndex];
  var provider = opt ? opt.getAttribute('data-provider') : '';
  var targetModel = opt ? opt.getAttribute('data-target') : '';
  var message = document.getElementById('pg-input').value.trim();
  if (!provider) { toast('Select a model first', 'error'); return; }
  if (!message) return;

  pgSending = true;
  document.getElementById('pg-send-btn').disabled = true;
  document.getElementById('pg-input').value = '';

  var chat = document.getElementById('pg-chat');
  var empty = chat.querySelector('.pg-empty');
  if (empty) empty.remove();

  appendMsg('user', escapeHtml(message));

  var assEl = appendMsg('assistant', '');
  var bodyEl = assEl.querySelector('.pg-msg-bubble');
  bodyEl.classList.add('pg-typing');

  fetch(API + '/api/playground', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ provider: provider, model: targetModel || model, message: message })
  }).then(function(r) {
    if (!r.ok) return r.json().then(function(d) { throw new Error(d.error || 'Request failed'); });
    bodyEl.classList.remove('pg-typing');
    return r.json();
  }).then(function(data) {
    var text = data.body || '';
    // Try parse as JSON for structured response
    try {
      var parsed = JSON.parse(text);
      if (parsed.choices && parsed.choices[0] && parsed.choices[0].message) {
        text = parsed.choices[0].message.content || '';
      } else if (parsed.content) {
        text = Array.isArray(parsed.content) ? parsed.content.map(function(b) { return b.text || ''; }).join('') : parsed.content;
      }
    } catch(e) {}
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

function escapeHtml(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }

// --- Init ---
loadStats();
loadPresets();
loadRoutes();

(function() {
  var hash = location.hash.replace('#', '');
  if (hash && document.getElementById('panel-' + hash)) {
    switchTab(hash);
  }
})();
