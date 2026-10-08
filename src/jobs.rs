use crate::memory::EventObserver;
use crate::runtime::{RunOutput, RunRequest, Workspace};
use crate::storage::{now_millis, read_json, validate_id, write_json, Lease};
use crate::tools::ExecutionControl;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub created_at: u64,
    pub due_at: u64,
    pub interval_ms: Option<u64>,
    pub status: String,
    pub request: RunRequest,
    pub run_id: Option<String>,
    pub last_run_status: Option<String>,
    pub result: Option<RunOutput>,
    pub error: Option<String>,
    pub cancel_requested: bool,
}

#[derive(Debug, Clone)]
pub struct Jobs {
    pub workspace: Workspace,
}

impl Jobs {
    fn path(&self, id: &str) -> Result<std::path::PathBuf> {
        validate_id(id)?;
        Ok(self
            .workspace
            .store
            .root
            .join("jobs")
            .join(format!("{id}.json")))
    }
    fn lock(&self) -> Result<Lease> {
        Lease::acquire_wait(
            &self.workspace.store.root.join(".jobs.lock"),
            Duration::from_secs(2),
        )
    }
    pub fn runner_lease(&self) -> Result<Lease> {
        Lease::acquire(&self.workspace.store.root.join(".runner.lock"))
    }
    pub fn get(&self, id: &str) -> Result<Job> {
        read_json(&self.path(id)?)
    }
    pub fn list(&self) -> Result<Vec<Job>> {
        let directory = self.workspace.store.root.join("jobs");
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut jobs = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                jobs.push(read_json::<Job>(&path)?);
            }
        }
        jobs.sort_by(|a, b| a.due_at.cmp(&b.due_at).then_with(|| a.id.cmp(&b.id)));
        Ok(jobs)
    }
    pub fn add(
        &self,
        mut request: RunRequest,
        due_at: u64,
        interval: Option<Duration>,
        isolated: bool,
    ) -> Result<Job> {
        let id = uuid::Uuid::new_v4().to_string();
        if isolated {
            request.session = format!("job_{}", uuid::Uuid::new_v4().simple());
        }
        let request = self.workspace.prepare_request(request)?;
        let interval_ms = interval
            .map(|interval| -> Result<u64> {
                let millis = u64::try_from(interval.as_millis())?;
                if millis < 100 {
                    bail!("job interval must be at least 100ms");
                }
                Ok(millis)
            })
            .transpose()?;
        // Validate assets before accepting scheduled work. No robot commands run here.
        self.workspace.validate_skills()?;
        let job = Job {
            id,
            created_at: now_millis(),
            due_at,
            interval_ms,
            status: "queued".into(),
            request,
            run_id: None,
            last_run_status: None,
            result: None,
            error: None,
            cancel_requested: false,
        };
        let _lease = self.lock()?;
        write_json(&self.path(&job.id)?, &job)?;
        Ok(job)
    }
    pub fn cancel(&self, id: &str) -> Result<Job> {
        let _lease = self.lock()?;
        let mut job = self.get(id)?;
        if job.status == "queued" {
            job.status = "cancelled".into();
        }
        if job.status == "running" || job.status == "cancelled" {
            job.cancel_requested = true;
        }
        write_json(&self.path(id)?, &job)?;
        Ok(job)
    }
    /// Call while holding the runner lease. Interrupted work is never auto-replayed.
    pub fn recover_interrupted(&self) -> Result<usize> {
        let _lease = self.lock()?;
        let mut recovered = 0;
        for mut job in self.list()? {
            if job.status == "running" {
                job.status = "interrupted".into();
                job.error = Some(
                    "runner ended without a terminal result; inspect state before submitting again"
                        .into(),
                );
                write_json(&self.path(&job.id)?, &job)?;
                recovered += 1;
            }
        }
        for mut run in self.workspace.store.runs()? {
            if run.status == "running" {
                let path = self
                    .workspace
                    .store
                    .session_dir(&run.session)?
                    .join(".session.lock");
                if let Ok(_lease) = Lease::acquire(&path) {
                    run.status = "interrupted".into();
                    run.finished_at = Some(now_millis());
                    run.error = Some(
                        "process ended without a terminal result; execution was not replayed"
                            .into(),
                    );
                    write_json(
                        &self.workspace.store.run_dir(&run.id)?.join("run.json"),
                        &run,
                    )?;
                }
            }
        }
        Ok(recovered)
    }
    /// Persist a claim before execution so process death cannot cause automatic replay.
    pub fn claim_due(&self, now: u64) -> Result<Option<Job>> {
        self.claim_due_excluding(now, &[])
    }

    pub fn claim_due_excluding(&self, now: u64, excluded: &[String]) -> Result<Option<Job>> {
        let _lease = self.lock()?;
        let mut job =
            match self.list()?.into_iter().find(|job| {
                job.status == "queued" && job.due_at <= now && !excluded.contains(&job.id)
            }) {
                Some(job) => job,
                None => return Ok(None),
            };
        job.status = "running".into();
        job.result = None;
        job.error = None;
        job.run_id = Some(uuid::Uuid::new_v4().to_string());
        write_json(&self.path(&job.id)?, &job)?;
        Ok(Some(job))
    }
    pub fn execute(
        &self,
        job: Job,
        shutdown: &ExecutionControl,
        observer: Option<EventObserver>,
    ) -> Result<Job> {
        let current = self.get(&job.id)?;
        if job.status != "running"
            || current.status != "running"
            || job.run_id.is_none()
            || job.run_id != current.run_id
        {
            bail!("job execution requires its current durable running claim");
        }
        let control = self.workspace.control(&job.request)?;
        if shutdown.stop_reason().is_some() || self.get(&job.id)?.cancel_requested {
            control.cancel();
        }
        let done = AtomicBool::new(false);
        struct Completion<'a>(&'a AtomicBool);
        impl Drop for Completion<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let execution = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !done.load(Ordering::Acquire) {
                    if shutdown.stop_reason().is_some()
                        || self
                            .get(&job.id)
                            .map(|job| job.cancel_requested)
                            .unwrap_or(true)
                    {
                        control.cancel();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
            let _completion = Completion(&done);
            self.workspace.run_with_id(
                job.request.clone(),
                &control,
                observer,
                None,
                job.run_id.as_deref(),
            )
        });
        let _lease = self.lock()?;
        // Re-read cancellation so a late cancel also prevents future interval runs.
        let mut stored = self.get(&job.id)?;
        match execution {
            Ok(result) => {
                stored.status = result.execution.status.as_str().into();
                stored.result = Some(result);
                stored.error = None;
            }
            Err(error) => {
                stored.status = "failed".into();
                stored.result = None;
                stored.error = Some(format!("{error:#}"));
            }
        }
        stored.last_run_status = Some(stored.status.clone());
        if stored.status == "completed"
            && !stored.cancel_requested
            && shutdown.stop_reason().is_none()
        {
            if let Some(interval) = stored.interval_ms {
                stored.due_at = now_millis()
                    .checked_add(interval)
                    .ok_or_else(|| anyhow::anyhow!("next job deadline overflow"))?;
                stored.status = "queued".into();
            }
        }
        write_json(&self.path(&job.id)?, &stored)?;
        Ok(stored)
    }
}
