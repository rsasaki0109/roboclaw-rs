'use strict';
const byId = id => document.getElementById(id);
let token = '', selectedJob = null, activeRun = null, after = 0, polling = false;
byId('timezone').value = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
async function api(path, data) {
  const response = await fetch(`/api/${path}`, {method: data === undefined ? 'GET' : 'POST', headers: {Authorization: `Bearer ${token}`, 'Content-Type': 'application/json'}, body: data === undefined ? undefined : JSON.stringify(data)});
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || `Request failed (${response.status})`);
  return result;
}
function showError(error) { byId('error').textContent = error.message; }
byId('connect').addEventListener('submit', async event => {
  event.preventDefault(); token = byId('token').value; byId('error').textContent = '';
  try { const health = await api('health'); byId('connection').textContent = `Connected · ${health.backend}`; await refresh(); }
  catch (error) { token = ''; byId('connection').textContent = 'Disconnected'; showError(error); }
});
byId('task').addEventListener('submit', async event => {
  event.preventDefault(); byId('error').textContent = '';
  const request = {instruction: byId('instruction').value, session: byId('session').value};
  try {
    if (event.submitter.value === 'plan') renderPlan(await api('plan', request));
    else {
      const submission = {request, isolated: false};
      if (event.submitter.value === 'schedule') {
        submission.cron = byId('cron').value.trim(); submission.timezone = byId('timezone').value.trim();
        if (!submission.cron || !submission.timezone) throw new Error('Enter a cron schedule and timezone.');
      }
      const job = await api('jobs', submission); selectJob(job); await refresh();
    }
  } catch (error) { showError(error); }
});
function selectJob(job) {
  selectedJob = job.id; activeRun = null; after = 0; byId('events').textContent = ''; byId('selected').textContent = `${job.request.session} · ${job.id}`;
}
async function refresh() {
  if (!token || polling) return;
  polling = true;
  try {
    const [jobs, deliveries] = await Promise.all([api('jobs'), api('webhooks')]);
    renderNotifications(deliveries);
    byId('jobs').replaceChildren();
    for (const job of [...jobs].reverse()) {
      const row = document.createElement('div'); row.className = 'job';
      const label = document.createElement('button'); label.className = 'job-label';
      const schedule = job.cron ? ` · ${job.cron.expression} (${job.cron.timezone})` : '';
      const due = job.status === 'queued' ? ` · Next: ${new Date(job.due_at).toLocaleString(undefined, job.cron ? {timeZone: job.cron.timezone, timeZoneName: 'short'} : {})}` : '';
      label.textContent = `${job.status} · ${job.request.session} · ${job.request.instruction}${schedule}${due}`; label.addEventListener('click', () => selectJob(job)); row.append(label);
      if (['queued', 'running'].includes(job.status)) {
        const cancel = document.createElement('button'); cancel.textContent = 'Cancel'; cancel.addEventListener('click', async () => { try { await api(`jobs/${job.id}/cancel`, {}); await refresh(); } catch (error) { showError(error); } }); row.append(cancel);
      }
      byId('jobs').append(row);
    }
    const observedJob = selectedJob;
    const job = jobs.find(job => job.id === observedJob);
    if (job && job.run_id) {
      if (job.run_id !== activeRun) { activeRun = job.run_id; after = 0; byId('events').textContent = ''; }
      const run = activeRun;
      const events = await api(`runs/${run}/events?after=${after}`);
      if (selectedJob !== observedJob || activeRun !== run) return;
      for (const event of events) { byId('events').textContent += `${event.seq} ${event.kind} ${JSON.stringify(event.payload)}\n`; after = event.seq; }
      const state = job.result?.backend_state;
      byId('selected').textContent = `${job.status} · ${job.request.session}${state ? ` · Pose: ${state.last_pose} · Holding: ${state.held_object || 'nothing'}` : ''}`;
    }
  } catch (error) { showError(error); } finally { polling = false; }
}
byId('inspect').addEventListener('click', async () => {
  try { const [doctor, skills] = await Promise.all([api('doctor'), api('skills')]); renderHealth(doctor, skills); } catch (error) { showError(error); }
});
setInterval(refresh, 1000);

function element(tag, text) { const node = document.createElement(tag); node.textContent = text; return node; }
function rawDetails(value) {
  const details = document.createElement('details'); details.append(element('summary', 'Structured output'), element('pre', JSON.stringify(value, null, 2))); return details;
}
function renderPlan(plan) {
  const container = byId('plan'); container.replaceChildren();
  container.append(element('h3', plan.decision.skill.name), element('p', `${plan.planner_provider} · ${plan.decision.reason || plan.decision.skill.description}`));
  const steps = document.createElement('ol');
  for (const step of plan.decision.skill.steps) {
    const row = document.createElement('li'); row.append(element('strong', step.name.replaceAll('_', ' ')), element('span', ` · ${step.tool}`), element('pre', JSON.stringify(step.input)));
    steps.append(row);
  }
  container.append(steps, rawDetails(plan));
}
function renderHealth(doctor, skills) {
  const container = byId('health'); container.replaceChildren();
  for (const check of doctor.checks) {
    const row = element('p', `${check.ok ? '✓' : '!'} ${check.name} · ${check.detail}`); row.className = check.ok ? 'check-ok' : 'check-error'; container.append(row);
  }
  container.append(element('h3', 'Available skills'));
  const list = document.createElement('ul');
  for (const skill of skills) list.append(element('li', `${skill.name} · ${skill.description}`));
  container.append(list, rawDetails({doctor, skills}));
}
function renderNotifications(deliveries) {
  const container = byId('webhooks'); container.replaceChildren();
  if (!deliveries.length) { container.append(element('p', 'No run notifications. Configure a webhook to notify new runs.')); return; }
  for (const delivery of [...deliveries].reverse()) {
    const row = element('p', `${delivery.status} · ${delivery.payload.session} · ${delivery.run_id} · Attempts: ${delivery.attempts}${delivery.error ? ` · ${delivery.error}` : ''}`);
    row.className = delivery.status === 'failed' ? 'check-error' : ''; container.append(row);
  }
}
