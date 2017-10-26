use conduit::{Request, Response};
use conduit_router::RequestParams;
use diesel::prelude::*;

use db::RequestTransaction;
use pagination::Paginate;
use util::{CargoResult, RequestUtils};

use api_types::keyword::EncodableKeyword;
use models::keyword::Keyword;

/// Handles the `GET /keywords` route.
pub fn index(req: &mut Request) -> CargoResult<Response> {
    use schema::keywords;

    let conn = req.db_conn()?;
    let (offset, limit) = req.pagination(10, 100)?;
    let query = req.query();
    let sort = query.get("sort").map(|s| &s[..]).unwrap_or("alpha");

    let mut query = keywords::table.into_boxed();

    if sort == "crates" {
        query = query.order(keywords::crates_cnt.desc());
    } else {
        query = query.order(keywords::keyword.asc());
    }

    let data = query.paginate(limit, offset).load::<(Keyword, i64)>(&*conn)?;
    let total = data.get(0).map(|&(_, t)| t).unwrap_or(0);
    let kws = data.into_iter()
        .map(|(k, _)| k.encodable())
        .collect::<Vec<_>>();

    #[derive(Serialize)]
    struct R {
        keywords: Vec<EncodableKeyword>,
        meta: Meta,
    }
    #[derive(Serialize)]
    struct Meta {
        total: i64,
    }

    Ok(req.json(&R {
        keywords: kws,
        meta: Meta { total: total },
    }))
}

/// Handles the `GET /keywords/:keyword_id` route.
pub fn show(req: &mut Request) -> CargoResult<Response> {
    let name = &req.params()["keyword_id"];
    let conn = req.db_conn()?;

    let kw = Keyword::find_by_keyword(&conn, name)?;

    #[derive(Serialize)]
    struct R {
        keyword: EncodableKeyword,
    }
    Ok(req.json(&R {
        keyword: kw.encodable(),
    }))
}
