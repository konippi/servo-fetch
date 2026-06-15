//! Response types for the HTTP API. Request types are the canonical wire DTOs
//! from `servo_fetch_types`.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct FetchResponse {
    pub url: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub(super) struct BatchFetchResponse {
    pub results: Vec<FetchResponse>,
}

#[derive(Debug, Serialize)]
pub(super) struct CrawlResponse {
    pub results: Vec<FetchResponse>,
}

#[derive(Debug, Serialize)]
pub(super) struct MapResponse {
    pub urls: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct HealthResponse {
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct VersionResponse {
    pub name: &'static str,
    pub version: &'static str,
}
