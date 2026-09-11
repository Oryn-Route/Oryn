//! Structured logging initialisation for the API server.
//!
//! This module provides backwards-compatible `init()` that delegates to
//! the new `tracing_config` module for distributed tracing support.
//!
//! # Environment variables
//!
//! | Variable                      | Values              | Default        |
//! |------------------------------|---------------------|----------------|
//! | `RUST_LOG`                   | tracing filter spec | `info`         |
//! | `LOG_FORMAT`                 | `json` \| `pretty`  | `pretty`       |
//! | `OTEL_EXPORTER_OTLP_ENDPOINT`| OTLP collector URL  | (disabled)     |
//! | `OTEL_SERVICE_NAME`          | Service name        | `orynroute` |
//! | `OTEL_SAMPLING_RATIO`        | 0.0 to 1.0          | `1.0`          |
//!
//! ## Examples
//!
//! ```bash
//! # Development
//! RUST_LOG=orynroute_api=debug ./orynroute-api
//!
//! # Production with OTLP export
//! RUST_LOG=info LOG_FORMAT=json OTEL_EXPORTER_OTLP_ENDPOINT=http://collector:4317 ./orynroute-api
//! ```

pub use crate::tracing_config::{init, init_with_config, shutdown, TracingConfig};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = TracingConfig::default();
        assert_eq!(config.service_name, "orynroute");
    }
}
