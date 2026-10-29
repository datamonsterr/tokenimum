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
function render() {
  element('title').textContent = selected[0].toUpperCase() + selected.slice(1);
  const pre = document.createElement('pre');
  pre.textContent = display(snapshot[selected] ?? []);
  element('content').replaceChildren(pre);
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
