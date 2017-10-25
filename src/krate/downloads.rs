//! Endpoint for exposing crate download counts
//!
//! The enpoints for download a crate and exposing version specific
//! download counts are located in `krate::downloads`.

use std::cmp;

use conduit::{Request, Response};
use conduit_router::RequestParams;
use diesel::prelude::*;

use db::RequestTransaction;
use download::{EncodableVersionDownload, VersionDownload};
use schema::*;
use util::{CargoResult, RequestUtils};
use models::Version;

use super::{to_char, Crate};

/// Handles the `GET /crates/:crate_id/downloads` route.
pub fn downloads(req: &mut Request) -> CargoResult<Response> {
    use diesel::dsl::*;
    use diesel::types::BigInt;

    let crate_name = &req.params()["crate_id"];
    let conn = req.db_conn()?;
    let krate = Crate::by_name(crate_name).first::<Crate>(&*conn)?;

    let mut versions = Version::belonging_to(&krate).load::<Version>(&*conn)?;
    versions.sort_by(|a, b| b.num.cmp(&a.num));
    let (latest_five, rest) = versions.split_at(cmp::min(5, versions.len()));

    let downloads = VersionDownload::belonging_to(latest_five)
        .filter(version_downloads::date.gt(date(now - 90.days())))
        .order(version_downloads::date.asc())
        .load(&*conn)?
        .into_iter()
        .map(VersionDownload::encodable)
        .collect::<Vec<_>>();

    let sum_downloads = sql::<BigInt>("SUM(version_downloads.downloads)");
    let extra = VersionDownload::belonging_to(rest)
        .select((
            to_char(version_downloads::date, "YYYY-MM-DD"),
            sum_downloads,
        ))
        .filter(version_downloads::date.gt(date(now - 90.days())))
        .group_by(version_downloads::date)
        .order(version_downloads::date.asc())
        .load::<ExtraDownload>(&*conn)?;

    #[derive(Serialize, Queryable)]
    struct ExtraDownload {
        date: String,
        downloads: i64,
    }
    #[derive(Serialize)]
    struct R {
        version_downloads: Vec<EncodableVersionDownload>,
        meta: Meta,
    }
    #[derive(Serialize)]
    struct Meta {
        extra_downloads: Vec<ExtraDownload>,
    }
    let meta = Meta {
        extra_downloads: extra,
    };
    Ok(req.json(&R {
        version_downloads: downloads,
        meta: meta,
    }))
}
