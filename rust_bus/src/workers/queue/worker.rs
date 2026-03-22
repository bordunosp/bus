use crate::BusError;
use crate::dispatch::registration::DATABASE_HANDLERS_BY_HASH;
use crate::sql::dto::BusJob;
use crate::sql::{
    fetch_jobs, handler_error, handler_error_shutdown, handler_success, recover_timed_out_jobs,
};
use crate::workers::configuration::BusQueueConfiguration;
use rand::RngExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use uuid::Uuid;

pub async fn start(shutdown_rx: watch::Receiver<()>) -> Result<(), BusError> {
    let queues = BusQueueConfiguration::global()?.queues();

    let recovery_rx = shutdown_rx.clone();
    tokio::spawn(async move {
        Worker::start_recovery_supervisor(recovery_rx).await;
    });

    for (queue_name, config) in queues {
        let name = queue_name.clone();
        let workers = config.workers as usize;
        let empty_queue_delay = config.empty_queue_delay;
        let rx = shutdown_rx.clone();

        #[cfg(feature = "logging")]
        log::info!("[Bus] Starting worker supervisor for queue: {}", name);

        tokio::spawn(async move {
            Worker::start_worker_supervisor(&name, workers, empty_queue_delay, rx).await;
        });
    }

    #[cfg(feature = "logging")]
    log::info!("[Bus] System initialized: workers and recovery are running.");

    Ok(())
}

pub async fn wait_or_shutdown(rx: &mut watch::Receiver<()>, duration: Duration) -> bool {
    tokio::select! {
        _ = rx.changed() => true,
        _ = tokio::time::sleep(duration) => false,
    }
}

pub(crate) struct Worker {
    worker_id: Uuid,
    queue_name: String,
    concurrency: usize,
    empty_queue_delay: Duration,
    shutdown_rx: watch::Receiver<()>,
}

impl Worker {
    pub fn new(
        queue_name: &str,
        concurrency: usize,
        empty_queue_delay: Duration,
        shutdown_rx: watch::Receiver<()>,
    ) -> Self {
        Self {
            worker_id: Uuid::now_v7(),
            queue_name: queue_name.to_string(),
            concurrency,
            empty_queue_delay,
            shutdown_rx,
        }
    }

    async fn start_recovery_supervisor(shutdown_rx: watch::Receiver<()>) {
        loop {
            let mut sig_rx = shutdown_rx.clone();

            let recovery_process = async {
                let mut interval = tokio::time::interval(Duration::from_secs(300));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                #[cfg(feature = "logging")]
                log::info!("[Manager] Recovery Worker started.");

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if let Err(_e) = recover_timed_out_jobs().await {
                                #[cfg(feature = "logging")]
                                log::error!("[Recovery] Error in recovery loop: {:?}", _e);
                            }
                        }
                        _ = sig_rx.changed() => break,
                    }
                }
            };

            let run_future = std::panic::AssertUnwindSafe(recovery_process);
            let result = futures::FutureExt::catch_unwind(run_future).await;

            match result {
                Ok(_) => {
                    #[cfg(feature = "logging")]
                    log::info!("[Manager] Recovery Worker exited normally.");
                    break;
                }
                Err(_panic_err) => {
                    #[cfg(feature = "logging")]
                    log::error!("[Manager] Recovery Worker CRASHED. Restarting in 10s...");

                    let mut delay_rx = shutdown_rx.clone();
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(10)) => continue,
                        _ = delay_rx.changed() => break,
                    }
                }
            }
        }
    }

    async fn start_worker_supervisor(
        queue_name: &str,
        concurrency: usize,
        empty_queue_delay: Duration,
        shutdown_rx: watch::Receiver<()>,
    ) {
        loop {
            let worker = Worker::new(
                queue_name,
                concurrency,
                empty_queue_delay,
                shutdown_rx.clone(),
            );
            let run_future = std::panic::AssertUnwindSafe(worker.run());
            let result = futures::FutureExt::catch_unwind(run_future).await;

            match result {
                Ok(_) => {
                    #[cfg(feature = "logging")]
                    log::info!("[Manager] Worker for {} exited normally.", queue_name);
                    break;
                }
                Err(_panic_err) => {
                    #[cfg(feature = "logging")]
                    log::error!(
                        "[Manager] Worker for {} CRASHED (panic). Restarting in 5s... Panic info: {:?}",
                        queue_name,
                        _panic_err
                    );

                    let mut sig_rx = shutdown_rx.clone();
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                        _ = sig_rx.changed() => break,
                    }
                }
            }
        }
    }

    #[cfg(feature = "logging")]
    fn process_task_result(
        res: Option<
            Result<Result<(), Box<dyn std::error::Error + Send + Sync>>, tokio::task::JoinError>,
        >,
        id: Uuid,
        queue: &str,
    ) {
        if let Some(res) = res {
            match res {
                Ok(Ok(_)) => log::debug!("[Worker {}][{}] Job completed", id, queue),
                Ok(Err(e)) => log::error!("[Worker {}][{}] Business error: {:?}", id, queue, e),
                Err(e) => log::error!("[Worker {}][{}] Task panicked: {:?}", id, queue, e),
            }
        }
    }

    async fn handler_execute(job: &BusJob) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let database_handlers = DATABASE_HANDLERS_BY_HASH.get().ok_or_else(|| {
            BusError::Configuration(
                "Bus not initialized. Call Bus::init() before dispatching.".to_string(),
            )
        })?;

        let reg = database_handlers.get(&job.hash_type_name).ok_or_else(|| {
            BusError::Configuration(format!(
                "Handler not found by handler {} and event {}",
                job.hash_type_name, job.type_name_event,
            ))
        })?;

        let db_pool = BusQueueConfiguration::global()?.get_connection();
        let db_ptr = db_pool as *const _ as usize;
        let meta_ptr = &job.meta as *const _ as usize;

        (reg.execute)(db_ptr, &job.payload, meta_ptr).await?;
        Ok(())
    }

    pub(crate) async fn run(self) {
        let concurrency = self.concurrency;
        let empty_delay = self.empty_queue_delay;
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let (tx, mut rx) = mpsc::channel::<BusJob>(concurrency);

        let worker_arc = Arc::new(self);
        let sem_c = Arc::clone(&semaphore);
        let mut active_jobs: std::collections::HashMap<Uuid, BusJob> =
            std::collections::HashMap::new();
        let mut join_set: JoinSet<Result<(Uuid, ()), Box<dyn std::error::Error + Send + Sync>>> =
            JoinSet::new();
        let queue_name_shared: Arc<str> = Arc::from(worker_arc.queue_name.as_str());
        let queue_for_producer = Arc::clone(&queue_name_shared);

        let mut shutdown_rx_consumer = worker_arc.shutdown_rx.clone();
        let mut shutdown_rx_producer = worker_arc.shutdown_rx.clone();
        let mut shutdown_rx_final = worker_arc.shutdown_rx.clone();

        #[cfg(feature = "logging")]
        let id = worker_arc.worker_id;

        #[cfg(feature = "logging")]
        let queue_for_consumer = Arc::clone(&queue_name_shared);

        let mut consumer_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx_consumer.changed() => {
                        #[cfg(feature = "logging")]
                        log::info!("[Worker {}] Shutdown received. Cleaning up {} active tasks...", id, join_set.len());
                        break;
                    },

                    result = join_set.join_next(), if !join_set.is_empty() => {
                        if let Some(Ok(Ok((job_id, _)))) = result {
                            active_jobs.remove(&job_id);
                        }

                        #[cfg(feature = "logging")]
                        {
                            let adapted_res = result.map(|res| res.map(|inner| inner.map(|_| ())));
                            Self::process_task_result(adapted_res, id, &queue_for_consumer);
                        }
                    }

                    Ok(permit) = sem_c.clone().acquire_owned(), if join_set.len() < concurrency => {
                        match rx.recv().await {
                            Some(job) => {
                                let job_id = job.id;
                                active_jobs.insert(job_id, job.clone());

                                join_set.spawn(async move {
                                    let _permit = permit;
                                    let execution_result = Self::execute_with_timeout(job.clone()).await;

                                    match execution_result {
                                        Ok(Ok(_)) => {
                                            if let Err(_e) = Self::with_db_retry(|| handler_success(job.id)).await {
                                                #[cfg(feature = "logging")]
                                                log::error!("[Worker {}] CRITICAL: Success flag failed for job {}: {:?}", id, job.id, _e);
                                            }
                                        }
                                        Ok(Err(err)) => {
                                            let err_msg = err.to_string();

                                            #[cfg(feature = "logging")]
                                            log::warn!("[Worker {}] Job {} failed: {}", id, job.id, err_msg);

                                            if let Err(_e) = Self::with_db_retry(|| handler_error(&job, err_msg.clone())).await {
                                                #[cfg(feature = "logging")]
                                                log::error!("[Worker {}] CRITICAL: Error reporting failed for job {}: {:?}", id, job.id, _e);
                                            }
                                        }
                                        Err(critical_msg) => {
                                            #[cfg(feature = "logging")]
                                            log::error!("[Worker {}] Job {} CRITICAL: {}", id, job.id, critical_msg);

                                            if let Err(_e) = Self::with_db_retry(|| handler_error(&job, critical_msg.clone())).await {
                                                #[cfg(feature = "logging")]
                                                log::error!("[Worker {}] Critical error update failed for job {}: {:?}", id, job.id, _e);
                                            }
                                        }
                                    }

                                    Ok((job_id, ()))
                                });
                            }
                            None => break,
                        }
                    }
                }
            }

            join_set.abort_all();

            for (_, job) in active_jobs.drain() {
                #[cfg(feature = "logging")]
                let job_id = job.id;

                #[cfg(feature = "logging")]
                log::warn!(
                    "[Worker] Marking job {} as Available due to shutdown",
                    job.id
                );

                if let Err(_e) = Self::with_db_retry(|| handler_error_shutdown(&job)).await {
                    #[cfg(feature = "logging")]
                    log::error!(
                        "[Worker {}] CRITICAL: Failed to return job {} to queue during shutdown after retries: {:?}",
                        id,
                        job_id,
                        _e
                    );
                }
            }

            #[cfg(feature = "logging")]
            log::info!(
                "[Worker {}][{}] Finalizing {} active tasks...",
                id,
                &queue_for_consumer,
                join_set.len()
            );
            while let Some(_res) = join_set.join_next().await {
                #[cfg(feature = "logging")]
                {
                    let adapted_res = Some(_res).map(|r| r.map(|inner| inner.map(|_| ())));
                    Self::process_task_result(adapted_res, id, &queue_for_consumer);
                }
            }
        });

        let tx_p = tx.clone();

        let mut producer_handle = tokio::spawn(async move {
            loop {
                let limit = tx_p.capacity();
                if limit == 0 {
                    tokio::select! {
                        _ = shutdown_rx_producer.changed() => break,
                        _ = tokio::time::sleep(Duration::from_millis(50)) => continue,
                    }
                }

                match fetch_jobs(&queue_for_producer, limit).await {
                    Ok(jobs) if jobs.is_empty() => {
                        let jitter = Duration::from_millis(rand::rng().random_range(0..200));
                        let sleep_dur = empty_delay + jitter;

                        tokio::select! {
                            _ = shutdown_rx_producer.changed() => break,
                            _ = tokio::time::sleep(sleep_dur) => {}
                        }
                    }

                    Ok(jobs) => {
                        for job in jobs {
                            if tx_p.send(job).await.is_err() {
                                return;
                            }

                            tokio::time::sleep(Duration::from_millis(2)).await;
                        }
                    }
                    Err(_e) => {
                        #[cfg(feature = "logging")]
                        log::error!("[Worker {}] DB Error: {}", id, _e);
                        tokio::select! {
                            _ = shutdown_rx_producer.changed() => break,
                            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                        }
                    }
                }
            }
        });

        tokio::select! {
            _ = shutdown_rx_final.changed() => {},
            _ = &mut consumer_handle => {},
            _ = &mut producer_handle => {},
        }

        drop(tx);
        let _ = tokio::join!(consumer_handle, producer_handle);
    }

    async fn with_db_retry<F, Fut, T>(mut op: F) -> Result<T, BusError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, BusError>>,
    {
        let mut last_err = None;
        for attempt in 1..=3 {
            match op().await {
                Ok(val) => return Ok(val),
                Err(e) => {
                    #[cfg(feature = "logging")]
                    log::warn!("[Worker] DB retry {}/3 failed: {:?}", attempt, e);

                    last_err = Some(e);
                    if attempt < 3 {
                        let base = 100 * 2_u64.pow(attempt - 1);
                        let jitter = rand::rng().random_range(0..50);
                        let delay = Duration::from_millis(base + jitter);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or(BusError::Database("Retry limit reached".into())))
    }

    pub(crate) async fn execute_with_timeout(
        job: BusJob,
    ) -> Result<Result<(), Box<dyn std::error::Error + Send + Sync>>, String> {
        let timeout_secs = job.execution_timeout_sec;
        let timeout_duration = Duration::from_secs(timeout_secs as u64);
        let job_for_task = job.clone();

        let mut handle: JoinHandle<
            Result<
                Result<(), Box<dyn std::error::Error + Send + Sync>>,
                Box<dyn std::any::Any + Send>,
            >,
        > = tokio::spawn(async move {
            futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async move {
                Self::handler_execute(&job_for_task).await
            }))
            .await
        });

        tokio::select! {
            res = &mut handle => {
                match res {
                    Ok(catch_unwind_result) => {
                        match catch_unwind_result {
                            Ok(handler_result) => Ok(handler_result),
                            Err(panic_payload) => {
                                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                                    s.to_string()
                                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "Unknown panic".to_string()
                                };
                                Err(msg)
                            }
                        }
                    }
                    Err(join_err) => Err(format!("Runtime join error: {}", join_err)),
                }
            }
            _ = tokio::time::sleep(timeout_duration) => {
                handle.abort();
                Err(format!("Job execution timed out after {}s", timeout_secs))
            }
        }
    }
}
