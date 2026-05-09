//! Custom `Json` extractor that returns [`ApiError`] on rejection so all error
//! responses share a consistent `application/json` envelope.

use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;

use super::error::ApiError;

pub(super) struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let axum::Json(value) = axum::Json::<T>::from_request(req, state).await?;
        Ok(Self(value))
    }
}
