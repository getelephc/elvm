use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::RETRY_AFTER;
use std::time::{Duration, SystemTime};

const MAX_ATTEMPTS: u32 = 6;
const MAX_DELAY: Duration = Duration::from_secs(60);

pub fn client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("elvm/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        // Blocking responses apply this timeout to each read, so an active
        // archive download can take longer than 30 seconds in total.
        .timeout(Duration::from_secs(30))
        .build()?)
}

#[derive(Debug, thiserror::Error)]
#[error("{source}")]
struct TransientError {
    #[source]
    source: anyhow::Error,
    retry_after: Option<Duration>,
}

pub fn transient(error: impl Into<anyhow::Error>) -> anyhow::Error {
    TransientError {
        source: error.into(),
        retry_after: None,
    }
    .into()
}

/// Retry the complete GET operation, including reads of its response body.
/// Disk errors, invalid metadata, and permanent HTTP errors are not retried.
pub fn retry<T>(attempt: impl FnMut() -> anyhow::Result<T>) -> anyhow::Result<T> {
    retry_with_sleep(attempt, std::thread::sleep)
}

fn retry_with_sleep<T>(
    mut attempt: impl FnMut() -> anyhow::Result<T>,
    mut sleep: impl FnMut(Duration),
) -> anyhow::Result<T> {
    for number in 0..MAX_ATTEMPTS {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) => {
                let Some(transient) = error.downcast_ref::<TransientError>() else {
                    return Err(error);
                };
                if number + 1 == MAX_ATTEMPTS {
                    return Err(error.context(format!("failed after {MAX_ATTEMPTS} attempts")));
                }
                let delay = transient
                    .retry_after
                    .unwrap_or_else(|| Duration::from_secs(1 << number));
                if delay > MAX_DELAY {
                    return Err(error.context(format!(
                        "server requested a {}s wait; please try again later",
                        delay.as_secs()
                    )));
                }
                eprintln!(
                    "network request failed: {error}; retrying in {}s",
                    delay.as_secs()
                );
                sleep(delay);
            }
        }
    }
    unreachable!()
}

/// Leave permanent statuses to callers, including GitHub's special 403/404 handling.
pub fn send(request: RequestBuilder) -> anyhow::Result<Response> {
    let response = request.send().map_err(|error| {
        if error.is_connect() || error.is_timeout() || error.is_body() || error.is_request() {
            transient(error)
        } else {
            error.into()
        }
    })?;
    let status = response.status();
    if status.is_server_error() || status.as_u16() == 408 || status.as_u16() == 429 {
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| retry_after(value, SystemTime::now()));
        return Err(TransientError {
            source: response.error_for_status().unwrap_err().into(),
            retry_after,
        }
        .into());
    }
    Ok(response)
}

pub fn text(response: Response) -> anyhow::Result<String> {
    response.text().map_err(transient)
}

fn retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    value
        .parse::<u64>()
        .map(Duration::from_secs)
        .ok()
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|date| date.duration_since(now).unwrap_or_default())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_are_bounded_and_use_exponential_backoff() {
        let mut attempts = 0;
        let mut delays = Vec::new();
        let result: anyhow::Result<()> = retry_with_sleep(
            || {
                attempts += 1;
                Err(transient(anyhow::anyhow!("connection lost")))
            },
            |delay| delays.push(delay.as_secs()),
        );
        assert!(result.unwrap_err().to_string().contains("6 attempts"));
        assert_eq!(attempts, 6);
        assert_eq!(delays, [1, 2, 4, 8, 16]);
    }

    #[test]
    fn permanent_errors_are_not_retried() {
        let mut attempts = 0;
        let result: anyhow::Result<()> = retry_with_sleep(
            || {
                attempts += 1;
                anyhow::bail!("disk full")
            },
            |_| panic!("must not sleep"),
        );
        assert!(result.is_err());
        assert_eq!(attempts, 1);
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates_and_limits_long_waits() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(retry_after("7", now), Some(Duration::from_secs(7)));
        let date = httpdate::fmt_http_date(now + Duration::from_secs(12));
        assert_eq!(retry_after(&date, now), Some(Duration::from_secs(12)));
        assert_eq!(retry_after("invalid", now), None);
        let mut attempts = 0;
        let mut delays = Vec::new();
        let result = retry_with_sleep(
            || {
                attempts += 1;
                if attempts == 1 {
                    Err(TransientError {
                        source: anyhow::anyhow!("rate limited"),
                        retry_after: Some(Duration::from_secs(3600)),
                    }
                    .into())
                } else {
                    Ok(())
                }
            },
            |delay| delays.push(delay),
        );
        assert!(result.unwrap_err().to_string().contains("try again later"));
        assert_eq!(attempts, 1);
        assert!(delays.is_empty());
    }
}
