use std::borrow::Cow;
use std::error::Error;
use std::fmt;

use super::{ChainElement, ErrorBuilder};
use crate::util::{json_response, AppResponse};

use chrono::NaiveDateTime;
use conduit::{header, StatusCode};

/// Generates a response with the provided status and description as JSON
fn json_error(detail: &str, status: StatusCode) -> AppResponse {
    #[derive(Serialize)]
    struct StringError<'a> {
        detail: &'a str,
    }
    #[derive(Serialize)]
    struct Bad<'a> {
        errors: Vec<StringError<'a>>,
    }

    let mut response = json_response(&Bad {
        errors: vec![StringError { detail }],
    });
    *response.status_mut() = status;
    response
}

// The following structs are emtpy and do not provide a custom message to the user

#[derive(Debug)]
pub(crate) struct NotFound;

impl Error for NotFound {}

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "NotFound".fmt(f)
    }
}

impl NotFound {
    pub(crate) fn response(&self) -> AppResponse {
        json_error("Not Found", StatusCode::NOT_FOUND)
    }

    #[cfg(test)]
    pub(crate) fn root_cause(&self) -> Box<ErrorBuilder> {
        Box::new(ErrorBuilder {
            chain: vec![ChainElement::Error(Box::new(Self))],
            user_facing_response: Some(self.response()),
        })
    }
}

#[derive(Debug)]
pub(crate) struct Forbidden;

impl Error for Forbidden {}

impl fmt::Display for Forbidden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "Forbidden".fmt(f)
    }
}

impl Forbidden {
    pub(crate) fn response(&self) -> AppResponse {
        let detail = "must be logged in to perform that action";
        json_error(detail, StatusCode::FORBIDDEN)
    }

    #[cfg(test)]
    pub(crate) fn root_cause(&self) -> Box<ErrorBuilder> {
        Box::new(ErrorBuilder {
            chain: vec![ChainElement::Error(Box::new(Self))],
            user_facing_response: Some(self.response()),
        })
    }
}

#[derive(Debug)]
pub(crate) struct ReadOnlyMode;

impl Error for ReadOnlyMode {}

impl fmt::Display for ReadOnlyMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "Tried to write in read only mode".fmt(f)
    }
}

impl ReadOnlyMode {
    pub(crate) fn response(&self) -> AppResponse {
        let detail = "Crates.io is currently in read-only mode for maintenance. \
                      Please try again later.";
        json_error(detail, StatusCode::SERVICE_UNAVAILABLE)
    }

    pub(crate) fn root_cause(&self) -> Box<ErrorBuilder> {
        Box::new(ErrorBuilder {
            chain: vec![ChainElement::Error(Box::new(Self))],
            user_facing_response: Some(self.response()),
        })
    }
}

// The following structs wrap owned data and provide a custom message to the user

#[derive(Debug)]
pub(super) struct CargoLegacy(pub(super) Cow<'static, str>);
#[derive(Debug)]
pub(super) struct BadRequest(pub(super) Cow<'static, str>);
#[derive(Debug)]
pub(super) struct ServerError(pub(super) &'static str);
#[derive(Debug)]
pub(crate) struct TooManyRequests {
    pub retry_after: NaiveDateTime,
}

impl CargoLegacy {
    pub(crate) fn response(&self) -> AppResponse {
        json_error(&self.0, StatusCode::OK)
    }
}

impl BadRequest {
    pub(crate) fn response(&self) -> AppResponse {
        json_error(&self.0, StatusCode::BAD_REQUEST)
    }
}

impl ServerError {
    pub(crate) fn response(&self) -> AppResponse {
        json_error(&self.0, StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl Error for TooManyRequests {}

impl fmt::Display for TooManyRequests {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "TooManyRequests".fmt(f)
    }
}

impl TooManyRequests {
    pub(crate) fn response(&self) -> AppResponse {
        use std::convert::TryInto;

        const HTTP_DATE_FORMAT: &str = "%a, %d %b %Y %H:%M:%S GMT";
        let retry_after = self.retry_after.format(HTTP_DATE_FORMAT);

        let detail = format!(
            "You have published too many crates in a \
             short period of time. Please try again after {} or email \
             help@crates.io to have your limit increased.",
            retry_after
        );
        let mut response = json_error(&detail, StatusCode::TOO_MANY_REQUESTS);
        response.headers_mut().insert(
            header::RETRY_AFTER,
            retry_after
                .to_string()
                .try_into()
                .expect("HTTP_DATE_FORMAT contains invalid char"),
        );
        response
    }

    pub(crate) fn root_cause(self) -> Box<ErrorBuilder> {
        Box::new(ErrorBuilder {
            user_facing_response: Some(self.response()),
            chain: vec![ChainElement::Error(Box::new(self))],
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct InsecurelyGeneratedTokenRevoked;

impl Error for InsecurelyGeneratedTokenRevoked {}

impl fmt::Display for InsecurelyGeneratedTokenRevoked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "insecurely generated, revoked 2020-07".fmt(f)
    }
}

impl InsecurelyGeneratedTokenRevoked {
    fn response(&self) -> AppResponse {
        let detail = "The given API token does not match the format used by crates.io. \
            \
            Tokens generated before 2020-07-14 were generated with an insecure \
            random number generator, and have been revoked. You can generate a \
            new token at https://crates.io/me. \
            \
            For more information please see \
            https://blog.rust-lang.org/2020/07/14/crates-io-security-advisory.html. \
            We apologize for any inconvenience.";
        json_error(detail, StatusCode::UNAUTHORIZED)
    }

    pub(crate) fn root_cause(&self) -> Box<ErrorBuilder> {
        Box::new(ErrorBuilder {
            chain: vec![ChainElement::Error(Box::new(Self))],
            user_facing_response: Some(self.response()),
        })
    }
}
