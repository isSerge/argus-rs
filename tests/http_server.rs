// Integration test root for http_server tests.
// Submodules live under `tests/http_server/` directory.

#[path = "http_server/helpers.rs"]
mod helpers;

#[path = "http_server/monitors.rs"]
mod monitors;

#[path = "http_server/actions.rs"]
mod actions;

#[path = "http_server/abis.rs"]
mod abis;

#[path = "http_server/status.rs"]
mod status;

#[path = "http_server/health.rs"]
mod health;
