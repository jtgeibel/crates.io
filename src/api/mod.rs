use std::sync::Arc;
use std::error::Error;

use conduit_router::RouteBuilder;
use conduit_middleware::MiddlewareBuilder;

use app::App;
use util::{R404, C, R};

use conduit;
use conduit_conditional_get;
use conduit_cookie;
use conduit_git_http_backend;
use conduit_log_requests;
use conduit_middleware;
use cookie;

use Env;
use local_upload;
use {app, dist, http, log, util};

use {category, crate_owner_invitation, krate, token, user};
mod keyword;
mod site_metadata;
mod version;

/// Configures routes, sessions, logging, and other middleware.
///
/// Called from *src/bin/server.rs*.
pub fn middleware(app: Arc<App>) -> MiddlewareBuilder {
    let mut api_router = RouteBuilder::new();

    // Route used by both `cargo search` and the frontend
    api_router.get("/crates", C(krate::search::search));

    // Routes used by `cargo`
    api_router.put("/crates/new", C(krate::publish::publish));
    api_router.get("/crates/:crate_id/owners", C(krate::owners::owners));
    api_router.put("/crates/:crate_id/owners", C(krate::owners::add_owners));
    api_router.delete("/crates/:crate_id/owners", C(krate::owners::remove_owners));
    api_router.delete("/crates/:crate_id/:version/yank", C(version::yank::yank));
    api_router.put(
        "/crates/:crate_id/:version/unyank",
        C(version::yank::unyank),
    );
    api_router.get(
        "/crates/:crate_id/:version/download",
        C(version::downloads::download),
    );

    // Routes that appear to be unused
    api_router.get("/versions", C(version::deprecated::index));
    api_router.get("/versions/:version_id", C(version::deprecated::show));

    // Routes used by the frontend
    api_router.get("/crates/:crate_id", C(krate::metadata::show));
    api_router.get("/crates/:crate_id/:version", C(version::deprecated::show));
    api_router.get(
        "/crates/:crate_id/:version/readme",
        C(krate::metadata::readme),
    );
    api_router.get(
        "/crates/:crate_id/:version/dependencies",
        C(version::metadata::dependencies),
    );
    api_router.get(
        "/crates/:crate_id/:version/downloads",
        C(version::downloads::downloads),
    );
    api_router.get(
        "/crates/:crate_id/:version/authors",
        C(version::metadata::authors),
    );
    api_router.get(
        "/crates/:crate_id/downloads",
        C(krate::downloads::downloads),
    );
    api_router.get("/crates/:crate_id/versions", C(krate::metadata::versions));
    api_router.put("/crates/:crate_id/follow", C(krate::follow::follow));
    api_router.delete("/crates/:crate_id/follow", C(krate::follow::unfollow));
    api_router.get("/crates/:crate_id/following", C(krate::follow::following));
    api_router.get("/crates/:crate_id/owner_team", C(krate::owners::owner_team));
    api_router.get("/crates/:crate_id/owner_user", C(krate::owners::owner_user));
    api_router.get(
        "/crates/:crate_id/reverse_dependencies",
        C(krate::metadata::reverse_dependencies),
    );
    api_router.get("/keywords", C(keyword::index));
    api_router.get("/keywords/:keyword_id", C(keyword::show));
    api_router.get("/categories", C(category::index));
    api_router.get("/categories/:category_id", C(category::show));
    api_router.get("/category_slugs", C(category::slugs));
    api_router.get("/users/:user_id", C(user::show));
    api_router.put("/users/:user_id", C(user::update_user));
    api_router.get("/users/:user_id/stats", C(user::stats));
    api_router.get("/teams/:team_id", C(user::show_team));
    api_router.get("/me", C(user::me));
    api_router.get("/me/updates", C(user::updates));
    api_router.get("/me/tokens", C(token::list));
    api_router.post("/me/tokens", C(token::new));
    api_router.delete("/me/tokens/:id", C(token::revoke));
    api_router.get(
        "/me/crate_owner_invitations",
        C(crate_owner_invitation::list),
    );
    api_router.put(
        "/me/crate_owner_invitations/:crate_id",
        C(crate_owner_invitation::handle_invite),
    );
    api_router.get("/summary", C(krate::metadata::summary));
    api_router.put("/confirm/:email_token", C(user::confirm_user_email));
    api_router.put("/users/:user_id/resend", C(user::regenerate_token_and_send));
    api_router.get("/site_metadata", C(site_metadata::show_deployed_sha));
    let api_router = Arc::new(R404(api_router));

    let mut router = RouteBuilder::new();

    // Mount the router under the /api/v1 path so we're at least somewhat at the
    // liberty to change things in the future!
    router.get("/api/v1/*path", R(Arc::clone(&api_router)));
    router.put("/api/v1/*path", R(Arc::clone(&api_router)));
    router.post("/api/v1/*path", R(Arc::clone(&api_router)));
    router.head("/api/v1/*path", R(Arc::clone(&api_router)));
    router.delete("/api/v1/*path", R(api_router));

    router.get("/authorize_url", C(user::github_authorize));
    router.get("/authorize", C(user::github_access_token));
    router.delete("/logout", C(user::logout));

    // Only serve the local checkout of the git index in development mode.
    // In production, for crates.io, cargo gets the index from
    // https://github.com/rust-lang/crates.io-index directly.
    let env = app.config.env;
    if env == Env::Development {
        let s = conduit_git_http_backend::Serve(app.git_repo_checkout.clone());
        let s = Arc::new(s);
        router.get("/git/index/*path", R(Arc::clone(&s)));
        router.post("/git/index/*path", R(s));
    }

    let mut m = MiddlewareBuilder::new(R404(router));

    if env == Env::Development {
        // DebugMiddleware is defined below to print logs for each request.
        m.add(DebugMiddleware);
        m.around(local_upload::Middleware::default());
    }

    if env != Env::Test {
        m.add(conduit_log_requests::LogRequests(log::LogLevel::Info));
    }

    m.around(util::Head::default());
    m.add(conduit_conditional_get::ConditionalGet);
    m.add(conduit_cookie::Middleware::new());
    m.add(conduit_cookie::SessionMiddleware::new(
        "cargo_session",
        cookie::Key::from_master(app.session_key.as_bytes()),
        env == Env::Production,
    ));
    if env == Env::Production {
        m.add(http::SecurityHeadersMiddleware::new(&app.config.uploader));
    }
    m.add(app::AppMiddleware::new(app));

    // Sets the current user on each request.
    m.add(user::Middleware);

    // Serve the static files in the *dist* directory, which are the frontend assets.
    // Not needed for the backend tests.
    if env != Env::Test {
        m.around(dist::Middleware::default());
    }

    return m;

    struct DebugMiddleware;

    impl conduit_middleware::Middleware for DebugMiddleware {
        fn before(&self, req: &mut conduit::Request) -> Result<(), Box<Error + Send>> {
            println!("  version: {}", req.http_version());
            println!("  method: {:?}", req.method());
            println!("  scheme: {:?}", req.scheme());
            println!("  host: {:?}", req.host());
            println!("  path: {}", req.path());
            println!("  query_string: {:?}", req.query_string());
            println!("  remote_addr: {:?}", req.remote_addr());
            for &(k, ref v) in &req.headers().all() {
                println!("  hdr: {}={:?}", k, v);
            }
            Ok(())
        }
        fn after(
            &self,
            _req: &mut conduit::Request,
            res: Result<conduit::Response, Box<Error + Send>>,
        ) -> Result<conduit::Response, Box<Error + Send>> {
            res.map(|res| {
                println!("  <- {:?}", res.status);
                for (k, v) in &res.headers {
                    println!("  <- {} {:?}", k, v);
                }
                res
            })
        }
    }
}
