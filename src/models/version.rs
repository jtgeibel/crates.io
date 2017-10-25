use std::collections::HashMap;

use chrono::NaiveDateTime;
use diesel;
use diesel::pg::Pg;
use diesel::prelude::*;
use semver;
use serde_json;

use Crate;
use dependency::Dependency;
use schema::*;
use util::{human, CargoResult};
use license_exprs;

use api_types::version::{EncodableVersion, VersionLinks};

// Queryable has a custom implementation below
#[derive(Clone, Identifiable, Associations, Debug)]
#[belongs_to(Crate)]
pub struct Version {
    pub id: i32,
    pub crate_id: i32,
    pub num: semver::Version,
    pub updated_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub downloads: i32,
    pub features: HashMap<String, Vec<String>>,
    pub yanked: bool,
    pub license: Option<String>,
}

#[derive(Insertable, Debug)]
#[table_name = "versions"]
pub struct NewVersion {
    crate_id: i32,
    num: String,
    features: String,
    license: Option<String>,
}

impl Version {
    pub fn encodable(self, crate_name: &str) -> EncodableVersion {
        let Version {
            id,
            num,
            updated_at,
            created_at,
            downloads,
            features,
            yanked,
            license,
            ..
        } = self;
        let num = num.to_string();
        EncodableVersion {
            dl_path: format!("/api/v1/crates/{}/{}/download", crate_name, num),
            readme_path: format!("/api/v1/crates/{}/{}/readme", crate_name, num),
            num: num.clone(),
            id: id,
            krate: crate_name.to_string(),
            updated_at: updated_at,
            created_at: created_at,
            downloads: downloads,
            features: features,
            yanked: yanked,
            license: license,
            links: VersionLinks {
                dependencies: format!("/api/v1/crates/{}/{}/dependencies", crate_name, num),
                version_downloads: format!("/api/v1/crates/{}/{}/downloads", crate_name, num),
                authors: format!("/api/v1/crates/{}/{}/authors", crate_name, num),
            },
        }
    }

    /// Returns (dependency, crate dependency name)
    pub fn dependencies(&self, conn: &PgConnection) -> QueryResult<Vec<(Dependency, String)>> {
        Dependency::belonging_to(self)
            .inner_join(crates::table)
            .select((dependencies::all_columns, crates::name))
            .order((dependencies::optional, crates::name))
            .load(conn)
    }

    pub fn max<T>(versions: T) -> semver::Version
    where
        T: IntoIterator<Item = semver::Version>,
    {
        versions.into_iter().max().unwrap_or_else(|| {
            semver::Version {
                major: 0,
                minor: 0,
                patch: 0,
                pre: vec![],
                build: vec![],
            }
        })
    }

    pub fn record_readme_rendering(&self, conn: &PgConnection) -> QueryResult<usize> {
        use schema::versions::dsl::readme_rendered_at;
        use diesel::dsl::now;

        diesel::update(self)
            .set(readme_rendered_at.eq(now.nullable()))
            .execute(conn)
    }
}

impl NewVersion {
    pub fn new(
        crate_id: i32,
        num: &semver::Version,
        features: &HashMap<String, Vec<String>>,
        license: Option<String>,
        license_file: Option<&str>,
    ) -> CargoResult<Self> {
        let features = serde_json::to_string(features)?;

        let mut new_version = NewVersion {
            crate_id: crate_id,
            num: num.to_string(),
            features: features,
            license: license,
        };

        new_version.validate_license(license_file)?;

        Ok(new_version)
    }

    pub fn save(&self, conn: &PgConnection, authors: &[String]) -> CargoResult<Version> {
        use diesel::{insert_into, select};
        use diesel::dsl::exists;
        use schema::versions::dsl::*;
        use schema::version_authors::{name, version_id};

        conn.transaction(|| {
            let already_uploaded = versions
                .filter(crate_id.eq(self.crate_id))
                .filter(num.eq(&self.num));
            if select(exists(already_uploaded)).get_result(conn)? {
                return Err(human(&format_args!(
                    "crate version `{}` is already \
                     uploaded",
                    self.num
                )));
            }

            let version = insert_into(versions)
                .values(self)
                .get_result::<Version>(conn)?;

            let new_authors = authors
                .iter()
                .map(|s| (version_id.eq(version.id), name.eq(s)))
                .collect::<Vec<_>>();

            insert_into(version_authors::table)
                .values(&new_authors)
                .execute(conn)?;
            Ok(version)
        })
    }

    fn validate_license(&mut self, license_file: Option<&str>) -> CargoResult<()> {
        if let Some(ref license) = self.license {
            for part in license.split('/') {
                license_exprs::validate_license_expr(part).map_err(|e| {
                    human(&format_args!(
                        "{}; see http://opensource.org/licenses \
                         for options, and http://spdx.org/licenses/ \
                         for their identifiers",
                        e
                    ))
                })?;
            }
        } else if license_file.is_some() {
            // If no license is given, but a license file is given, flag this
            // crate as having a nonstandard license. Note that we don't
            // actually do anything else with license_file currently.
            self.license = Some(String::from("non-standard"));
        }
        Ok(())
    }
}

impl Queryable<versions::SqlType, Pg> for Version {
    #[cfg_attr(feature = "clippy", allow(type_complexity))]
    type Row = (
        i32,
        i32,
        String,
        NaiveDateTime,
        NaiveDateTime,
        i32,
        Option<String>,
        bool,
        Option<String>,
        Option<NaiveDateTime>,
    );

    fn build(row: Self::Row) -> Self {
        let features = row.6
            .map(|s| serde_json::from_str(&s).unwrap())
            .unwrap_or_else(HashMap::new);
        Version {
            id: row.0,
            crate_id: row.1,
            num: semver::Version::parse(&row.2).unwrap(),
            updated_at: row.3,
            created_at: row.4,
            downloads: row.5,
            features: features,
            yanked: row.7,
            license: row.8,
        }
    }
}
