use crate::core::error_bus::BusError;
use crate::core::features::sea_orm::initialization::get_queue_config;
use crate::core::features::sea_orm::worker_completed_tasks_handler::handle_completed_tasks;
use crate::core::features::sea_orm::worker_expired_tasks_handler::handle_expired_tasks;
use crate::core::features::sea_orm::worker_failed_tasks_handler::handle_failed_tasks;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;

#[cfg(test)]
pub(crate) async fn process_batch(
    _: &crate::core::features::sea_orm::core::DatabaseQueueConfiguration,
) -> Result<bool, BusError> {
    tokio::time::sleep(Duration::from_secs(1)).await;
    Ok(true)
}

#[cfg(not(test))]
use crate::core::features::sea_orm::workers_process::process_batch;

struct WorkerContext {
    #[allow(dead_code)]
    pub queue_name: String,
    shutdown_rx: watch::Receiver<()>,
}

impl WorkerContext {
    pub fn new(queue_name: String, shutdown_rx: watch::Receiver<()>) -> Self {
        Self {
            queue_name,
            shutdown_rx,
        }
    }

    async fn wait_or_shutdown(&mut self, duration: Duration) -> bool {
        tokio::select! {
            _ = self.shutdown_rx.changed() => true,
            _ = tokio::time::sleep(duration) => false,
        }
    }

    #[cfg(test)]
    fn is_shutdown(&self) -> bool {
        self.shutdown_rx.has_changed().unwrap_or(true)
    }
}

#[allow(dead_code)]
pub struct DatabaseWorkerController {
    shutdown_tx: watch::Sender<()>,
    handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

#[allow(dead_code)]
impl DatabaseWorkerController {
    pub fn new() -> Self {
        let (shutdown_tx, _) = watch::channel(());
        Self {
            shutdown_tx,
            handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn spawn_background_worker<F, Fut>(
        &self,
        _name: &'static str,
        sleep_duration: Duration,
        mut shutdown_rx: watch::Receiver<()>,
        task_fn: F,
    ) where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        let handles = self.handles.clone();
        let handle = tokio::spawn(async move {
            #[cfg(feature = "logging")]
            log::info!("Bus. Background worker '{}' started", _name);

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        #[cfg(feature = "logging")]
                        log::info!("Bus. Background worker '{}' shutting down", _name);
                        break;
                    }
                    result = task_fn() => {
                        match result {
                            Ok(did_work) => {
                                if !did_work {
                                    tokio::select! {
                                        _ = shutdown_rx.changed() => break,
                                        _ = tokio::time::sleep(sleep_duration) => {}
                                    }
                                }
                            }
                            Err(_e) => {
                                #[cfg(feature = "logging")]
                                log::error!("Bus. Error in '{}': {}", _name, _e);
                                tokio::select! {
                                    _ = shutdown_rx.changed() => break,
                                    _ = tokio::time::sleep(Duration::from_secs(600)) => {}
                                }
                            }
                        }
                    }
                }
            }
        });

        tokio::spawn(async move {
            handles.lock().await.push(handle);
        });
    }

    pub async fn start_workers(&self) -> Result<(), BusError> {
        let queues = get_queue_config()?.queues().clone();

        self.spawn_background_worker(
            "expired_tasks_handler",
            Duration::from_secs(60),
            self.shutdown_tx.subscribe(),
            handle_expired_tasks,
        );

        self.spawn_background_worker(
            "failed_tasks_handler",
            Duration::from_secs(300),
            self.shutdown_tx.subscribe(),
            handle_failed_tasks,
        );

        self.spawn_background_worker(
            "completed_tasks_handler",
            Duration::from_secs(300),
            self.shutdown_tx.subscribe(),
            handle_completed_tasks,
        );

        for queue in queues {
            let workers = queue.workers();

            #[cfg(feature = "logging")]
            log::debug!(
                "Bus. Starting {} workers for queue '{}'",
                workers,
                queue.queue_name()
            );

            for _i in 0..workers {
                let queue_clone = queue.clone();
                let handles = self.handles.clone();
                let shutdown_rx = self.shutdown_tx.subscribe();
                let mut ctx = WorkerContext::new(queue_clone.queue_name().to_string(), shutdown_rx);

                let handle = tokio::spawn(async move {
                    #[cfg(feature = "logging")]
                    log::info!(
                        "Bus. Worker #{} for '{}' started",
                        _i + 1,
                        ctx.queue_name.to_string()
                    );

                    loop {
                        tokio::select! {
                            _ = ctx.shutdown_rx.changed() => {
                                #[cfg(feature = "logging")]
                                log::info!("Bus. Shutting down worker for '{}'", ctx.queue_name.to_string());
                                break;
                            }
                            result = process_batch(&queue_clone) => {
                                match result {
                                    Ok(is_empty_queue) => {
                                        if is_empty_queue {
                                            if ctx.wait_or_shutdown(queue_clone.sleep_interval()).await {
                                                break;
                                            }
                                        }
                                    }
                                    Err(_e) => {
                                        #[cfg(feature = "logging")]
                                        log::error!("Bus. Worker error on '{}': {}", ctx.queue_name.to_string(), _e);
                                        if ctx.wait_or_shutdown(queue_clone.sleep_interval() * 2).await {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                });

                handles.lock().await.push(handle);
            }
        }

        Ok(())
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    pub async fn await_termination_with_timeout(&self, max_duration: Duration) {
        let mut handles = self.handles.lock().await;
        let join_all = futures::future::join_all(handles.drain(..));

        match timeout(max_duration, join_all).await {
            Ok(_) => {
                #[cfg(feature = "logging")]
                log::info!("Bus. All Database workers shut down gracefully.")
            }
            Err(_) => {
                #[cfg(feature = "logging")]
                log::warn!("Bus. Timeout reached. Some workers did not shut down in time.");
                for handle in handles.drain(..) {
                    handle.abort();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::features::sea_orm::core::{
        ArchiveType, DatabaseConnection, DatabaseQueueConfiguration,
        DatabaseQueueProcessorConfiguration,
    };
    use crate::core::features::sea_orm::dto::{
        DatabaseConnectionDto, DatabaseQueueConfigurationDto,
    };
    use crate::core::features::sea_orm::initialization::init_sea_orm_for_tests;
    use sea_orm::{ConnectOptions, DatabaseBackend, MockDatabase, MockExecResult};
    use std::time::Duration;

    fn setup_mock_config(queue_name: &str, workers: usize) -> DatabaseQueueProcessorConfiguration {
        let mock_conn = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 1,
                rows_affected: 1,
            }])
            .into_connection();

        let connect_options = ConnectOptions::new("sqlite::memory:");

        let db_config = DatabaseConnection::new(DatabaseConnectionDto {
            sea_orm_connect_options: connect_options,
            table_name: "bus_events".to_string(),
            table_name_archive: "bus_events_archive".to_string(),
        })
        .unwrap();

        let queue_config = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: queue_name.to_string(),
            workers,
            batch_size: 10,
            sleep_interval: Duration::from_millis(10),
            archive_type: ArchiveType::None,
        })
        .unwrap();

        let processor_config =
            DatabaseQueueProcessorConfiguration::new(db_config, vec![queue_config]);

        init_sea_orm_for_tests(mock_conn, processor_config.clone()).unwrap();
        processor_config
    }

    #[tokio::test]
    async fn test_controller_creation_and_shutdown_signal() {
        let controller = DatabaseWorkerController::new();
        let mut rx = controller.shutdown_tx.subscribe();
        controller.shutdown();
        let changed = rx.changed().await;
        assert!(changed.is_ok(), "Shutdown signal was not received");
    }

    #[tokio::test]
    async fn test_start_and_shutdown_workers() {
        setup_mock_config("test-queue", 2);
        let controller = DatabaseWorkerController::new();
        controller.start_workers().await.unwrap();
        controller.shutdown();
        controller
            .await_termination_with_timeout(Duration::from_secs(1))
            .await;
    }

    #[tokio::test]
    async fn test_shutdown_timeout_triggers_abort() {
        setup_mock_config("slow-queue", 1);
        let controller = Arc::new(DatabaseWorkerController::new());
        controller.start_workers().await.unwrap();

        let ctrl = controller.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            ctrl.shutdown();
        });

        controller
            .await_termination_with_timeout(Duration::from_secs(1))
            .await;
    }

    #[tokio::test]
    async fn test_worker_context_wait_or_shutdown_shutdown() {
        let (tx, rx) = watch::channel(());
        let mut ctx = WorkerContext::new("test".into(), rx);
        tx.send(()).unwrap();
        let exited = ctx.wait_or_shutdown(Duration::from_millis(100)).await;
        assert!(exited);
    }

    #[tokio::test]
    async fn test_worker_context_wait_or_shutdown_timeout() {
        let (_tx, rx) = watch::channel(());
        let mut ctx = WorkerContext::new("test".into(), rx);
        let exited = ctx.wait_or_shutdown(Duration::from_millis(10)).await;
        assert!(!exited);
    }

    #[tokio::test]
    async fn test_worker_context_is_shutdown() {
        let (tx, rx) = watch::channel(());
        let ctx = WorkerContext::new("test".into(), rx);
        assert!(!ctx.is_shutdown());
        tx.send(()).unwrap();
        assert!(ctx.is_shutdown());
    }
}

// pub async fn run_with_shutdown() -> Result<(), BusError> {
//     let controller = DatabaseWorkerController::new();
//     controller.start_workers().await?;
//
//     tokio::select! {
//         _ = tokio::signal::ctrl_c() => {
//             log::info!("Received Ctrl+C, initiating shutdown...");
//         }
//     }
//
//     controller.shutdown();
//     controller.await_termination_with_timeout(Duration::from_secs(10)).await;
//
//     Ok(())
// }
