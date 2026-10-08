'use strict';
const byId = id => document.getElementById(id);
let token = '', selectedJob = null, activeRun = null, after = 0, polling = false;
let selectedChallenge = null;
let renderedChallenge = null;
const retrying = new Set();
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
    const [jobs, deliveries, challenges] = await Promise.all([api('jobs'), api('webhooks'), api('challenges')]);
    renderChallenges(challenges);
    const observedChallenge = selectedChallenge;
    if (observedChallenge) {
      const report = await api(`challenges/${observedChallenge}`);
      if (selectedChallenge === observedChallenge) renderChallengeResults(report);
    }
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
    if ((job && job.run_id) || (!observedJob && activeRun)) {
      if (job && job.run_id !== activeRun) { activeRun = job.run_id; after = 0; byId('events').textContent = ''; }
      const run = activeRun;
      const events = await api(`runs/${run}/events?after=${after}`);
      if (selectedJob !== observedJob || activeRun !== run) return;
      for (const event of events) { byId('events').textContent += `${event.seq} ${event.kind} ${JSON.stringify(event.payload)}\n`; after = event.seq; }
      if (job) {
        const state = job.result?.backend_state;
        byId('selected').textContent = `${job.status} · ${job.request.session}${state ? ` · Pose: ${state.last_pose} · Holding: ${state.held_object || 'nothing'}` : ''}`;
      }
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
    const entry = document.createElement('div'); entry.className = 'notification';
    const row = element('p', `${delivery.status} · ${delivery.payload.session} · ${delivery.run_id} · Attempts: ${delivery.attempts}${delivery.error ? ` · ${delivery.error}` : ''}`);
    row.className = delivery.status === 'failed' ? 'check-error' : ''; entry.append(row);
    if (delivery.status === 'failed') {
      const retry = element('button', 'Retry notification'); retry.disabled = retrying.has(delivery.id);
      retry.addEventListener('click', async () => {
        if (retrying.has(delivery.id)) return;
        retrying.add(delivery.id); retry.disabled = true; byId('error').textContent = '';
        try { await api(`webhooks/${delivery.id}/retry`, {}); }
        catch (error) { showError(error); }
        finally { retrying.delete(delivery.id); await refresh(); }
      });
      entry.append(retry);
    }
    if (delivery.retries?.length) {
      const details = document.createElement('details'); details.append(element('summary', `Retry history (${delivery.retries.length})`));
      const history = document.createElement('ul');
      for (const previous of delivery.retries) {
        history.append(element('li', `${new Date(previous.requested_at).toLocaleString()} · Attempts: ${previous.attempts} · ${previous.error || 'Unacknowledged delivery'}`));
      }
      details.append(history); entry.append(details);
    }
    container.append(entry);
  }
}

byId('challenge-form').addEventListener('submit', async event => {
  event.preventDefault(); byId('error').textContent = '';
  const checked = name => [...document.querySelectorAll(`#challenge-form input[name="${name}"]:checked`)].map(input => input.value);
  const request = {scenarios: checked('scenario'), strategies: checked('strategy'), providers: checked('provider'), repeats: Number(byId('challenge-repeats').value), timeout: byId('challenge-timeout').value.trim()};
  byId('challenge-start').disabled = true;
  try { const challenge = await api('challenges', request); selectedChallenge = challenge.id; await refresh(); }
  catch (error) { showError(error); }
  finally { byId('challenge-start').disabled = false; }
});
function renderChallenges(challenges) {
  const container = byId('challenges'); container.replaceChildren();
  if (!challenges.length) { container.append(element('p', 'No comparisons yet. Start with Mock to try the stock skills.')); return; }
  for (const challenge of [...challenges].reverse()) {
    const row = element('div', ''); row.className = 'job';
    const finished = challenge.trials.filter(trial => !['queued', 'running'].includes(trial.status)).length;
    const label = element('button', `${challenge.status} · ${finished}/${challenge.trials.length} trials · ${new Date(challenge.created_at).toLocaleString()}`);
    label.className = 'job-label'; label.setAttribute('aria-pressed', String(selectedChallenge === challenge.id));
    label.addEventListener('click', async () => {
      selectedChallenge = challenge.id;
      try { const report = await api(`challenges/${challenge.id}`); if (selectedChallenge === challenge.id) renderChallengeResults(report); }
      catch (error) { showError(error); }
    }); row.append(label);
    if (['queued', 'running'].includes(challenge.status)) {
      const cancel = element('button', challenge.cancel_requested ? 'Stopping…' : 'Cancel comparison'); cancel.disabled = challenge.cancel_requested;
      cancel.addEventListener('click', async () => { try { await api(`challenges/${challenge.id}/cancel`, {}); await refresh(); } catch (error) { showError(error); } }); row.append(cancel);
    }
    container.append(row);
  }
}
function renderChallengeResults(report) {
  const {challenge, rankings} = report;
  const version = `${challenge.id}/${challenge.updated_at}/${challenge.status}`;
  if (renderedChallenge === version) return;
  renderedChallenge = version;
  const container = byId('challenge-results');
  const detailsOpen = container.querySelector('details')?.open || false;
  container.replaceChildren();
  container.append(element('h3', `Comparison · ${challenge.status}`));
  const wrap = element('div', ''); wrap.className = 'table-scroll';
  const table = document.createElement('table'); table.append(element('caption', 'Planner and strategy results'));
  const head = document.createElement('thead'), header = document.createElement('tr');
  for (const title of ['Planner / strategy', 'Passed / planned', 'Mean time', 'Retries', 'Replans']) { const th = element('th', title); th.scope = 'col'; header.append(th); }
  head.append(header); table.append(head);
  const body = document.createElement('tbody');
  for (const rank of rankings) {
    const row = document.createElement('tr');
    for (const text of [`${rank.provider} / ${rank.strategy}`, `${rank.passed}/${rank.planned} (${Math.round(rank.pass_rate * 100)}%)`, rank.mean_elapsed_ms === null ? '—' : `${Math.round(rank.mean_elapsed_ms)} ms`, rank.retries, rank.replans]) row.append(element('td', text));
    body.append(row);
  }
  table.append(body); wrap.append(table); container.append(wrap);
  container.append(element('p', 'Time includes planning and execution. “Completed” means the comparison finished; individual missions can fail. Sensor-wait trials pass when the deadline stops execution before motion.'));
  const details = document.createElement('details'); details.append(element('summary', `Trial details (${challenge.trials.length})`));
  details.open = detailsOpen;
  const list = document.createElement('ul');
  for (const trial of challenge.trials) {
    const row = element('li', `${trial.scenario} · ${trial.provider} / ${trial.strategy} · Repeat ${trial.repetition} · ${trial.status} · ${trial.passed ? 'PASS' : 'not passed'} · Faults: ${trial.faults_injected} · Retries: ${trial.retries}${trial.recovery_ms === null ? '' : ` · Recovery: ${trial.recovery_ms} ms`}${trial.error ? ` · ${trial.error}` : ''}`);
    if (trial.elapsed_ms !== null) {
      const trace = element('button', 'View events'); trace.className = 'trace-button';
      trace.addEventListener('click', async () => {
        selectedJob = null; activeRun = trial.run_id; after = 0; byId('events').textContent = ''; byId('selected').textContent = `${trial.scenario} · ${trial.status} · ${trial.run_id}`;
        await refresh(); byId('events').scrollIntoView({behavior: 'smooth', block: 'center'});
      }); row.append(trace);
    }
    list.append(row);
  }
  details.append(list); container.append(details);
}
