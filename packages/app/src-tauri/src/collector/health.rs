//! HTTP probe against the Collector's `health_check` extension.
//!
//! The bundled smoke configuration exposes the extension on
//! `127.0.0.1:13133/health`. A 200 response is the supervisor's signal
//! that the Collector finished initialising and is accepting OTLP traffic
//! on `:4317` / `:4318`.

use std::time::Duration;

use thiserror::Error;
use tokio::time::Instant;

#[derive(Debug, Error)]
pub enum HealthError {
    #[error("health endpoint did not return 200 within {timeout:?} (last status: {last_status:?})")]
    Timeout {
        timeout: Duration,
        last_status: Option<u16>,
    },
}

/// Polls `health_url` every `poll_interval` until it returns HTTP 200 or
/// the deadline elapses. Network errors are treated like a non-200
/// response — the supervisor doesn't care *why* the server isn't ready,
/// only that it isn't yet.
pub async fn wait_until_healthy(
    health_url: &str,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), HealthError> {
    let deadline = Instant::now() + timeout;
    let client = build_client();
    let mut last_status: Option<u16> = None;

    loop {
        if let Ok(resp) = client.get(health_url).send().await {
            let status = resp.status().as_u16();
            last_status = Some(status);
            if status == 200 {
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            return Err(HealthError::Timeout {
                timeout,
                last_status,
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Single non-blocking probe. Returns true on HTTP 200, false otherwise.
/// Used by Sprint 6's status dashboard for steady-state reporting.
#[allow(dead_code)]
pub async fn is_healthy(health_url: &str) -> bool {
    let client = build_client();
    matches!(
        client.get(health_url).send().await,
        Ok(resp) if resp.status() == 200
    )
}

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .no_proxy()
        .build()
        .expect("reqwest client construction is infallible for this configuration")
}
