//! Ensures that we returned a well formed response when we error, because civet vomits

use super::prelude::*;

#[derive(Default)]
pub struct EnsureWellFormed500;

impl Middleware for EnsureWellFormed500 {
    fn after(&self, _: &mut dyn RequestExt, res: AfterResult) -> AfterResult {
        res.or_else(|_| {
            let body = "Internal Server Error";
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CONTENT_LENGTH, body.len())
                .body(Body::from_static(body.as_bytes()))
                .map_err(box_error)
        })
    }
}
