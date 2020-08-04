//! This module implements several error types and traits.  The suggested usage in returned results
//! is as follows:
//!
//! * The concrete `util::concrete::Error` type (re-exported as `util::Error`) is great for code
//!   that is not part of the request/response lifecycle.  It avoids pulling in the unnecessary
//!   infrastructure to convert errors into a user facing JSON responses (relative to `AppError`).
//! * `diesel::QueryResult` - There is a lot of code that only deals with query errors.  If only
//!   one type of error is possible in a function, using that specific error is preferable to the
//!   more general `util::Error`.  This is especially common in model code.
//! * `util::errors::AppResult` - Some failures should be converted into user facing JSON
//!   responses.  This error type is more dynamic and is box allocated.  Low-level errors are
//!   typically not converted to user facing errors and most usage is within the models,
//!   controllers, and middleware layers.

use std::borrow::Cow;
use std::error::Error;
use std::fmt;

use diesel::result::Error as DieselError;

use crate::util::AppResponse;

pub(super) mod concrete;
mod json;

pub(crate) use json::{
    Forbidden, InsecurelyGeneratedTokenRevoked, NotFound, ReadOnlyMode, TooManyRequests,
};

pub type AppResult<T> = Result<T, Box<ErrorBuilder>>;

/// A struct with helper methods for common error responses.
pub(crate) struct UserFacing;

impl UserFacing {
    /// Returns an error with status 400 and the provided description as JSON.
    pub(crate) fn bad_request(user_message: &'static str) -> AppResponse {
        json::BadRequest(user_message.into()).response()
    }

    /// Return a custom error with status 400 and the provided description as JSON.
    ///
    /// Care should be taken not to include sensitive information when generating
    /// custom user facing messages.
    fn custom_bad_request(user_message: String) -> AppResponse {
        json::BadRequest(user_message.into()).response()
    }

    /// Returns an error with status 500 and the provided description as JSON.
    pub(crate) fn server_error(user_message: &'static str) -> AppResponse {
        json::ServerError(user_message).response()
    }

    /// Returns an error with status 200 and the provided user message as JSON.
    ///
    /// Newer versions of cargo support other status codes so usage of these helpers
    /// should be removed over time.
    pub(crate) fn cargo_err_legacy(user_message: &'static str) -> AppResponse {
        json::CargoLegacy(user_message.into()).response()
    }

    /// Returns an error with status 200 and the provided user message as JSON.
    ///
    /// Newer versions of cargo support other status codes so usage of these helpers
    /// should be removed over time.
    ///
    /// Care should be taken not to include sensitive information when generating
    /// custom user facing messages.
    fn custom_cargo_err_legacy(user_message: String) -> AppResponse {
        json::CargoLegacy(user_message.into()).response()
    }
}

/// A builder that maintains a chain of internal errors and a user-facing response.
pub struct ErrorBuilder {
    /// The cause chain, intended for logging and not the user.
    /// The first element, if present, is the root cause.
    chain: Vec<ChainElement>,
    /// An error response prepared for the user.
    user_facing_response: Option<AppResponse>,
}

impl fmt::Debug for ErrorBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErrorBuilder")
            .field("chain", &self.chain)
            .field(
                "user_facing_response",
                if self.user_facing_response.is_none() {
                    &"None"
                } else {
                    &"Some(_)"
                },
            )
            .finish()
    }
}

impl<E: Error + 'static> From<E> for Box<ErrorBuilder> {
    fn from(err: E) -> Self {
        convert_special_errors(err)
    }
}

impl ErrorBuilder {
    /// Create a builder for an error with status 400 and the provided description as JSON.
    pub(crate) fn bad_request(user_message: &'static str) -> Box<Self> {
        Box::new(ErrorBuilder {
            chain: vec![],
            user_facing_response: Some(UserFacing::bad_request(user_message)),
        })
    }

    /// Create a builder for an error with status 400 and the provided description as JSON.
    ///
    /// Care should be taken not to include sensitive information when generating
    /// custom user facing messages.
    pub(crate) fn custom_bad_request(user_message: String) -> Box<Self> {
        Box::new(ErrorBuilder {
            chain: vec![],
            user_facing_response: Some(UserFacing::custom_bad_request(user_message)),
        })
    }

    /// Create a builder for an error with status 500 and the provided description as JSON.
    pub(crate) fn server_error(user_message: &'static str) -> Box<Self> {
        Box::new(ErrorBuilder {
            chain: vec![],
            user_facing_response: Some(json::ServerError(user_message).response()),
        })
    }

    /// Create a builder with a root internal error and no initial user facing response.
    pub(crate) fn internal(info: Cow<'static, str>) -> Box<Self> {
        Box::new(Self {
            chain: vec![ChainElement::Internal(info)],
            user_facing_response: None,
        })
    }

    /// Create a builder for an error with status 200 and the provided user message as JSON.
    ///
    /// Newer versions of cargo support other status codes so usage of these helpers
    /// should be removed over time.
    pub(crate) fn cargo_err_legacy(user_message: &'static str) -> Box<Self> {
        Box::new(ErrorBuilder {
            chain: vec![],
            user_facing_response: Some(UserFacing::cargo_err_legacy(user_message)),
        })
    }

    /// Create a builder for an error with status 200 and the provided user message as JSON.
    ///
    /// Newer versions of cargo support other status codes so usage of these helpers
    /// should be removed over time.
    ///
    /// Care should be taken not to include sensitive information when generating
    /// custom user facing messages.
    pub(crate) fn custom_cargo_err_legacy(user_message: String) -> Box<Self> {
        Box::new(ErrorBuilder {
            chain: vec![],
            user_facing_response: Some(UserFacing::custom_cargo_err_legacy(user_message)),
        })
    }

    /// Test the error type of the root cause, if there is one.
    pub(crate) fn root_cause_is<T: Error + 'static>(&self) -> bool {
        self.chain
            .first()
            .map(|root| matches!(root, ChainElement::Error(e) if e.is::<T>()))
            .unwrap_or(false)
    }

    #[cfg(test)]
    pub(crate) fn into_response(self) -> Option<AppResponse> {
        self.user_facing_response
    }

    /// Generate a summary of the cause chain, appropriate for logging.
    fn cause_chain(&self) -> String {
        self.chain
            .iter()
            .rev()
            .map(|element| match element {
                ChainElement::Internal(info) => info.clone(),
                ChainElement::Error(e) => Cow::Owned(e.to_string()),
            })
            .collect::<Vec<_>>()
            .join(" caused by ")
    }

    /// Finalize the error response built by the endpoint.
    pub(crate) fn build(self) -> BuiltResponse {
        if self.user_facing_response.is_some() {
            let cause = if self.chain.is_empty() {
                None
            } else {
                Some(self.cause_chain())
            };
            return BuiltResponse::Response {
                // The unwrap is fine because user_facing_response is Some(_)
                response: self.user_facing_response.unwrap(),
                cause,
            };
        } else if let Some(ChainElement::Error(root_cause)) = self.chain.first() {
            // Convert database NotFound into a user-facing response
            if let Some(diesel::result::Error::NotFound) = root_cause.downcast_ref() {
                return BuiltResponse::Response {
                    response: NotFound.response(),
                    cause: None,
                };
            }
        }

        BuiltResponse::Error(Box::new(InternalAppError(self.cause_chain())))
    }
}

/// A type representing the elements of the cause chain.
#[derive(Debug)]
enum ChainElement {
    /// An internal error message.
    Internal(Cow<'static, str>),
    /// An error, most useful as the root cause (the first item) in the chain.
    Error(Box<dyn Error + 'static>),
}

/// A representation of the final error output of an endpoint.
pub(crate) enum BuiltResponse {
    /// A user-facing response with an optional cause for logging.
    Response {
        response: AppResponse,
        cause: Option<String>,
    },
    /// An error to propogate up the middleware stack when no user-facing response is available.
    /// The middleware stack will convert this to a generic Internal Server Error after logging
    /// this as `error="..."` using Display.
    Error(Box<dyn Error + Send>),
}

/// A trait providing helper methods for working with an `ErrorBuilder`.
pub(crate) trait ChainError<T> {
    /// Capture a user facing error response.
    ///
    /// The fallback is only applied if a user response has not yet been set. In general, an
    /// error prepared further down the call stack (and vetted as appropriate for providing to
    /// the user) should not be overwritten by a more generic error higher up the call stack.
    fn chain_user_facing_fallback(self, callback: fn() -> AppResponse) -> AppResult<T>;

    /// Capture an internal message for the cause chain that is logged
    ///
    /// The cause chain produces a string like "... caused by ..." with the innermost
    /// error appearing last.
    fn chain_internal_err_cause(self, cause: &'static str) -> AppResult<T>;
}

/// Convert some errors into user-facing responses when converted to a builder.
///
/// There are several places where a generic error is converted into an `ErrorBuilder`:
///
/// * A From<E> impl for `E: Error + 'static`, producing a Box<ErrorBuilder>.
/// * The `ChainError` methods for `Result<T, E>`.
fn convert_special_errors<E: Error + 'static>(cause: E) -> Box<ErrorBuilder> {
    match (&cause as &dyn Error).downcast_ref() {
        Some(DieselError::DatabaseError(_, info))
            if info.message().ends_with("read-only transaction") =>
        {
            ReadOnlyMode.root_cause()
        }
        // Cannot use the From impl here, because that would be recursive
        _ => Box::new(ErrorBuilder {
            chain: vec![ChainElement::Error(Box::new(cause))],
            user_facing_response: None,
        }),
    }
}

impl<T, E: Error + 'static> ChainError<T> for Result<T, E> {
    /// If the Result is an error, apply any special conversions then chain with the message.
    fn chain_internal_err_cause(self, internal_message: &'static str) -> AppResult<T> {
        self.or_else(|cause| {
            Err(convert_special_errors(cause)).chain_internal_err_cause(internal_message)
        })
    }

    /// If the result is an error, apply any special conversiona and add the user facing error.
    fn chain_user_facing_fallback(self, callback: fn() -> AppResponse) -> AppResult<T> {
        self.or_else(|cause| {
            Err(convert_special_errors(cause)).chain_user_facing_fallback(callback)
        })
    }
}

impl<T> ChainError<T> for AppResult<T> {
    /// Add an message to the cause chain.
    fn chain_internal_err_cause(self, internal_message: &'static str) -> Self {
        self.map_err(|mut builder| {
            builder
                .chain
                .push(ChainElement::Internal(internal_message.into()));
            builder
        })
    }

    /// Add a user-facing response if one has not yet been set on the builder.
    ///
    /// The callback is only called if a user response has not yet been provided.
    fn chain_user_facing_fallback(self, callback: fn() -> AppResponse) -> AppResult<T> {
        self.map_err(|mut builder| {
            if builder.user_facing_response.is_none() {
                builder.user_facing_response = Some(callback())
            };
            builder
        })
    }
}

impl<T> ChainError<T> for Option<T> {
    /// If the value is None, convert it to an error with the provided message.
    fn chain_internal_err_cause(self, internal_message: &'static str) -> AppResult<T> {
        self.ok_or_else(|| {
            Box::new(ErrorBuilder {
                chain: vec![ChainElement::Internal(internal_message.into())],
                user_facing_response: None,
            })
        })
    }

    /// If the value is None, the callback is invoked to generate a user facing error response.
    fn chain_user_facing_fallback(self, callback: fn() -> AppResponse) -> AppResult<T> {
        self.ok_or_else(|| {
            Box::new(ErrorBuilder {
                chain: vec![],
                user_facing_response: Some(callback()),
            })
        })
    }
}

/// Internal error to provide to the middlewhere when there is no user-facing response.
#[derive(Debug)]
struct InternalAppError(String);

impl Error for InternalAppError {}

impl fmt::Display for InternalAppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        Ok(())
    }
}

#[test]
fn chain_error_internal() {
    assert_eq!(
        None::<()>
            .chain_internal_err_cause("inner")
            .chain_internal_err_cause("middle")
            .chain_internal_err_cause("outer")
            .unwrap_err()
            .cause_chain(),
        "outer caused by middle caused by inner"
    );
    assert_eq!(
        Err::<(), _>(ErrorBuilder::internal("inner".into()))
            .chain_internal_err_cause("outer")
            .unwrap_err()
            .cause_chain(),
        "outer caused by inner"
    );
    assert_eq!(
        Err::<(), _>(ErrorBuilder::cargo_err_legacy("inner"))
            .chain_internal_err_cause("outer")
            .unwrap_err()
            .cause_chain(),
        "outer"
    );
    assert_eq!(
        Err::<(), _>(Forbidden.root_cause())
            .chain_internal_err_cause("outer")
            .unwrap_err()
            .cause_chain(),
        "outer caused by Forbidden"
    );
}

#[test]
fn chain_error_user_facing() {
    let response = Err::<(), _>(ErrorBuilder::cargo_err_legacy("inner"))
        .chain_user_facing_fallback(|| UserFacing::cargo_err_legacy("outer"))
        .unwrap_err()
        .build();

    match response {
        BuiltResponse::Response {
            response,
            cause: None,
        } => match response.into_body() {
            // The user sees the inner user-facing error response
            conduit::Body::Owned(bytes) => assert_eq!(bytes, br#"{"errors":[{"detail":"inner"}]}"#),
            _ => panic!("Unexpected response Body type"),
        },
        _ => panic!("Unexpected BuildResponse type"),
    }

    let response = Err::<(), _>(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        .chain_user_facing_fallback(|| UserFacing::cargo_err_legacy("outer"))
        .unwrap_err()
        .build();

    match response {
        BuiltResponse::Response {
            response,
            cause: Some(cause),
        } if cause == "permission denied" => match response.into_body() {
            // ^ The inner error is available for logging
            // The outer error is sent as a response to the client.
            conduit::Body::Owned(bytes) => assert_eq!(bytes, br#"{"errors":[{"detail":"outer"}]}"#),
            _ => panic!("Unexpected response body"),
        },
        _ => panic!("Unexpected response type"),
    }
}
