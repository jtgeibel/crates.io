//! This crate implements the backend server for https://crates.io/
//!
//! All implemented routes are defined in the [middleware](fn.middleware.html) function and
//! implemented in the [category](category/index.html), [keyword](keyword/index.html),
//! [krate](krate/index.html), [user](user/index.html) and [version](version/index.html) modules.
#![deny(warnings)]
#![deny(missing_debug_implementations, missing_copy_implementations)]
#![cfg_attr(feature = "clippy", feature(plugin))]
#![cfg_attr(feature = "clippy", plugin(clippy))]
#![recursion_limit = "128"]

extern crate ammonia;
extern crate chrono;
extern crate comrak;
extern crate curl;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_codegen;
extern crate diesel_full_text_search;
extern crate dotenv;
extern crate flate2;
extern crate git2;
extern crate hex;
extern crate lettre;
extern crate license_exprs;
#[macro_use]
extern crate log;
extern crate oauth2;
extern crate openssl;
extern crate r2d2;
extern crate r2d2_diesel;
extern crate rand;
extern crate s3;
extern crate semver;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate tar;
extern crate toml;
extern crate url;

extern crate conduit;
extern crate conduit_conditional_get;
extern crate conduit_cookie;
extern crate conduit_git_http_backend;
extern crate conduit_log_requests;
extern crate conduit_middleware;
extern crate conduit_router;
extern crate conduit_static;
extern crate cookie;

pub use app::App;
pub use self::badge::Badge;
pub use self::category::Category;
pub use config::Config;
pub use self::dependency::Dependency;
pub use self::download::VersionDownload;
pub use self::keyword::Keyword;
pub use self::krate::Crate;
pub use self::user::User;
pub use self::version::Version;
pub use self::uploaders::{Bomb, Uploader};

pub mod api;
pub mod app;
pub mod badge;
pub mod boot;
pub mod category;
pub mod config;
pub mod crate_owner_invitation;
pub mod db;
pub mod dependency;
pub mod dist;
pub mod download;
pub mod git;
pub mod github;
pub mod http;
pub mod keyword;
pub mod krate;
pub mod owner;
pub mod render;
pub mod schema;
pub mod token;
pub mod upload;
pub mod uploaders;
pub mod user;
pub mod util;
pub mod version;
pub mod email;

mod local_upload;
mod pagination;

/// Used for setting different values depending on whether the app is being run in production,
/// in development, or for testing.
///
/// The app's `config.env` value is set in *src/bin/server.rs* to `Production` if the environment
/// variable `HEROKU` is set and `Development` otherwise. `config.env` is set to `Test`
/// unconditionally in *src/test/all.rs*.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Env {
    Development,
    Test,
    Production,
}

/// Used for setting different values depending on the type of registry this instance is.
///
/// `Primary` indicates this instance is a primary registry that is the source of truth for these
/// crates' information. `ReadOnlyMirror` indicates this instanceis a read-only mirror of crate
/// information that exists on another instance.
///
/// The app's `config.mirror` value is set in *src/bin/server.rs* to `ReadOnlyMirror` if the
/// `MIRROR` environment variable is set and to `Primary` otherwise.
///
/// There may be more ways to run crates.io servers in the future, such as a
/// mirror that also has private crates that crates.io does not have.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Replica {
    Primary,
    ReadOnlyMirror,
}

/// Convenience function requiring that an environment variable is set.
///
/// Ensures that we've initialized the dotenv crate in order to read environment variables
/// from a *.env* file if present. Don't use this for optionally set environment variables.
///
/// # Panics
///
/// Panics if the environment variable with the name passed in as an argument is not defined
/// in the current environment.
pub fn env(s: &str) -> String {
    dotenv::dotenv().ok();
    ::std::env::var(s).unwrap_or_else(|_| panic!("must have `{}` defined", s))
}

sql_function!(lower, lower_t, (x: ::diesel::types::Text) -> ::diesel::types::Text);
