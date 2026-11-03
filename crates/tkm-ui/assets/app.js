'use strict';
const token = new URLSearchParams(location.hash.slice(1)).get('token') || '';
const panes = ['sessions', 'decisions', 'spend', 'savings', 'facts', 'redirects'];
let selected = 'sessions';
let snapshot = {};
const element = id => document.getElementById(id);
async function api(path, options = {}) {
  const headers = {Authorization: `Bearer ${token}`, ...options.headers};
  const response = await fetch(path, {...options, headers});
  if (!response.ok) throw new Error(`Request failed: ${response.status}`);
  return response.json();
}
function display(value) { return JSON.stringify(value, (key, item) => item === null && (key === "priced_cost" || key === "cost") ? "unknown" : item, 2); }
function emptyMessage(pane) {
  switch (pane) {
    case 'sessions': return 'No active sessions. Start an agent session in a repository where `tkm init` installed hooks.';
    case 'decisions': return snapshot.decisions_unavailable
      ? 'No decisions yet. Decisions are off: set decide.enabled = true and configure an OpenRouter key (tkm models probe).'
      : 'No decisions yet.';
    case 'spend': return 'No decisions yet; per-model share appears here.';
    case 'savings': return 'No priced sessions yet. Savings compare session cost per repository once sessions end with a known price.';
    case 'facts': return 'No facts yet; facts are learned from decisions over several sessions.';
    case 'redirects': return 'No redirects yet. Redirects appear when the loop guard steers an agent (loop.mode is shadow or on).';
    default: return 'Nothing to show yet.';
  }
}
function renderSpend() {
  const spend = snapshot.spend;
  const section = document.createElement('section');
  const summary = document.createElement('p');
  summary.textContent = spend ? `Today: $${spend.today_usd.toFixed(6)} / $${spend.daily_cap_usd.toFixed(2)} daily cap; session cap $${spend.session_cap_usd.toFixed(2)}` : 'Today: unknown';
  section.append(summary);
  const totals = new Map();
  for (const decision of snapshot.decisions || []) totals.set(decision.model, (totals.get(decision.model) || 0) + decision.cost_usd);
  const total = [...totals.values()].reduce((sum, amount) => sum + amount, 0);
  const shares = document.createElement('pre');
  shares.textContent = [...totals].map(([model, amount]) => `${model}: $${amount.toFixed(6)} (${total ? (100 * amount / total).toFixed(1) : '0.0'}% of observed decisions)`).join('\n') || emptyMessage('spend');
  if (snapshot.unknown_decision_costs) shares.textContent += `\nDecision cost unknown: ${snapshot.unknown_decision_costs} attempts`;
  section.append(shares);
  const values = (snapshot.spend_days || []).slice(-7);
  const max = Math.max(1, ...values.map(([, amount]) => amount));
  const points = !values.some(([, amount]) => amount > 0) ? '' : values.map(([, amount], index) => `${values.length > 1 ? index * 590 / (values.length - 1) : 0},${90 - amount * 80 / max}`).join(' ');
  const svg = document.createElementNS('http:' + '/' + '/www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 600 100'); svg.setAttribute('role', 'img'); svg.setAttribute('aria-label', 'Seven day spend sparkline');
  const line = document.createElementNS(svg.namespaceURI, 'polyline');
  line.setAttribute('points', points); line.setAttribute('fill', 'none'); line.setAttribute('stroke', 'currentColor');
  svg.append(line); section.append(svg);
  element('content').replaceChildren(section);
}
function render() {
  element('title').textContent = selected[0].toUpperCase() + selected.slice(1);
  if (selected === 'spend') { renderSpend(); }
  else {
  const pre = document.createElement('pre');
  const items = snapshot[selected] ?? [];
  pre.textContent = Array.isArray(items) && items.length === 0 && !snapshot.unknown_session_costs ? emptyMessage(selected) : display(items);
  element('content').replaceChildren(pre);
  }
  for (const button of element('tabs').children) button.setAttribute('aria-selected', String(button.dataset.pane === selected));
}
for (const pane of panes) {
  const button = document.createElement('button');
  button.textContent = pane[0].toUpperCase() + pane.slice(1);
  button.dataset.pane = pane;
  button.onclick = () => { selected = pane; render(); };
  element('tabs').append(button);
}
async function refresh() {
  try {
    snapshot = await api('/api/state'); render();
    element('status').textContent = 'Connected';
    if (snapshot.unknown_decision_costs) element('status').textContent += `; decision cost unknown for ${snapshot.unknown_decision_costs} attempts`;
    const history = await api('/api/history');
    const days = (history.days || []).filter(day => day.cost !== null);
    if (snapshot.unknown_session_costs) element('status').textContent += `; session cost unknown for ${snapshot.unknown_session_costs} sessions`;
    const peak = Math.max(1, ...days.map(day => day.cost));
    const points = days.map((day, i) => `${days.length > 1 ? i * 590 / (days.length - 1) : 0},${110 - day.cost * 100 / peak}`).join(' ');
    const line = document.createElementNS('http:' + '/' + '/www.w3.org/2000/svg', 'polyline');
    line.setAttribute('points', points); line.setAttribute('fill', 'none'); line.setAttribute('stroke', 'currentColor');
    element('chart').replaceChildren(line);
  } catch (error) { element('status').textContent = error.message; }
}
element('fact').onsubmit = async event => {
  event.preventDefault(); const data = new FormData(event.target);
  try { element('evidence').textContent = display(await api(`/api/facts/${encodeURIComponent(data.get('repo'))}/${encodeURIComponent(data.get('fact_id'))}`)); } catch (error) { element('evidence').textContent = error.message; }
};
element('prune').onclick = async () => { try { element('evidence').textContent = display(await api('/api/prune')); } catch (error) { element('evidence').textContent = error.message; } };
element('action').onsubmit = async event => {
  event.preventDefault(); const data = new FormData(event.target); const kind = data.get('kind');
  const payload = {repo: data.get('repo'), checkout: data.get('repo'), fact_id: data.get('item'), tool: data.get('item'), host: data.get('host'), preset: data.get('preset')};
  try {
    const result = await api(`/api/actions/${kind}`, {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify(payload)});
    if (window.confirm(result.summary)) { const done = await api(`/api/confirm/${result.confirm_id}`, {method: 'POST'}); element('status').textContent = done.summary; await refresh(); }
  } catch (error) { element('status').textContent = error.message; }
};
refresh(); setInterval(refresh, 2000);
