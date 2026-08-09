//! Global session configuration validation.

use std::time::Duration;

use servo_fetch::{SessionBrokerConfig, WorkerCommand, configure_default_broker, set_default_worker_command};

#[test]
fn invalid_global_configuration_does_not_consume_once_lock() {
    assert!(set_default_worker_command(WorkerCommand::new("relative-worker")).is_err());
    let executable = std::env::current_exe().unwrap();
    set_default_worker_command(WorkerCommand::new(executable)).unwrap();

    assert!(configure_default_broker(SessionBrokerConfig::default().max_sessions(0)).is_err());
    configure_default_broker(
        SessionBrokerConfig::default()
            .max_sessions(1)
            .queue_capacity(1)
            .prewarm(0)
            .acquire_timeout(Duration::from_secs(1)),
    )
    .unwrap();
}
