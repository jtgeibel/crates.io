use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::ops::{Deref, DerefMut};
use std::time::Duration;
use std::thread::{self, ThreadId};

use conduit::RequestExt;
use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager, CustomizeConnection};
use url::Url;

use crate::middleware::app::RequestApp;
use crate::Env;

use crossbeam::channel::{self, Receiver, Sender};

#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub enum DieselPool {
    Pool(r2d2::Pool<ConnectionManager<PgConnection>>),
    Test(FakeSendSync<PgConnection>),
}

impl DieselPool {
    #[track_caller]
    pub fn get(&self) -> Result<DieselPooledConn, r2d2::PoolError> {
        match self {
            DieselPool::Pool(pool) => Ok(DieselPooledConn::Pool(pool.get()?)),
            DieselPool::Test(conn) => {
                debug!("DieselPool::get");
                //let conn = rx.recv_timeout(Duration::from_millis(1000)).unwrap();
                Ok(DieselPooledConn::Test(conn.clone()))
                //Ok(DieselPooledConn::Test(conn.lock().unwrap().take().unwrap()))//.expect("multiple attemtps to get a connection from the pool, but tests only have 1 connection")))
            }
        }
    }

    pub fn state(&self) -> r2d2::State {
        match self {
            DieselPool::Pool(pool) => pool.state(),
            DieselPool::Test { .. } => panic!("Cannot get the state of a test pool"),
        }
    }

    fn test_conn(conn: PgConnection) -> Self {
        //let (tx, rx) = channel::bounded(1);
        //tx.send(conn).unwrap();
        DieselPool::Test(FakeSendSync::new(conn))
    }
}

#[allow(missing_debug_implementations)]
pub enum DieselPooledConn {
    Pool(r2d2::PooledConnection<ConnectionManager<PgConnection>>),
    Test(FakeSendSync<PgConnection>),
}

//unsafe impl<'a> Send for DieselPooledConn<'a> {}

//impl Drop for DieselPooledConn {
//    fn drop(&mut self) {
//        match self {
//            DieselPooledConn::Pool(_) => (),
//            DieselPooledConn::Test { tx, conn } => {
//                debug!("DieselPooledConn::drop()");
//                let conn = conn.take().expect("somebody stole the test connection");
//                tx.send(conn).unwrap();
//            }
//        }
//    }
//}

impl Deref for DieselPooledConn {
    type Target = PgConnection;

    #[track_caller]
    fn deref(&self) -> &Self::Target {
        match self {
            DieselPooledConn::Pool(conn) => conn.deref(),
            DieselPooledConn::Test(conn) => conn.deref(),
        }
    }
}

pub fn connect_now() -> ConnectionResult<PgConnection> {
    let url = connection_url(&crate::env("DATABASE_URL"));
    PgConnection::establish(&url)
}

pub fn connection_url(url: &str) -> String {
    let mut url = Url::parse(url).expect("Invalid database URL");
    if dotenv::var("HEROKU").is_ok() && !url.query_pairs().any(|(k, _)| k == "sslmode") {
        url.query_pairs_mut().append_pair("sslmode", "require");
    }
    url.into_string()
}

pub fn diesel_pool(
    url: &str,
    env: Env,
    config: r2d2::Builder<ConnectionManager<PgConnection>>,
) -> DieselPool {
    let url = connection_url(url);
    if env == Env::Test {
        let conn = PgConnection::establish(&url).expect("failed to establish connection");
        DieselPool::test_conn(conn)
    } else {
        let manager = ConnectionManager::new(url);
        DieselPool::Pool(config.build(manager).unwrap())
    }
}

pub trait RequestTransaction {
    /// Obtain a read/write database connection from the primary pool
    fn db_conn(&self) -> Result<DieselPooledConn, r2d2::PoolError>;

    /// Obtain a readonly database connection from the replica pool
    ///
    /// If there is no replica pool, the primary pool is used instead.
    fn db_read_only(&self) -> Result<DieselPooledConn, r2d2::PoolError>;
}

impl<T: RequestExt + ?Sized> RequestTransaction for T {
    #[track_caller]
    fn db_conn(&self) -> Result<DieselPooledConn, r2d2::PoolError> {
        let conn = self.app().primary_database.get().map_err(Into::into)?;
        // self.mut_extensions().insert(conn);
        //Ok(&*self.extensions().find::<&PgConnection>().unwrap())
        Ok(conn)
    }

    #[track_caller]
    fn db_read_only(&self) -> Result<DieselPooledConn, r2d2::PoolError> {
        match &self.app().read_only_replica_database {
            Some(pool) => pool.get().map_err(Into::into),
            None => self.app().primary_database.get().map_err(Into::into),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ConnectionConfig {
    pub statement_timeout: u64,
    pub read_only: bool,
}

impl CustomizeConnection<PgConnection, r2d2::Error> for ConnectionConfig {
    fn on_acquire(&self, conn: &mut PgConnection) -> Result<(), r2d2::Error> {
        use diesel::sql_query;

        sql_query(format!(
            "SET statement_timeout = {}",
            self.statement_timeout * 1000
        ))
        .execute(conn)
        .map_err(r2d2::Error::QueryError)?;
        if self.read_only {
            sql_query("SET default_transaction_read_only = 't'")
                .execute(conn)
                .map_err(r2d2::Error::QueryError)?;
        }
        Ok(())
    }
}

#[allow(missing_debug_implementations)]
pub struct FakeSendSync<T> {
    test_thread_id: ThreadId,
    other_thread_id: Option<ThreadId>,
    value: Arc<T>,
}

unsafe impl<T> Send for FakeSendSync<T> {}
unsafe impl<T> Sync for FakeSendSync<T> {}

impl<T> FakeSendSync<T> {
    fn new(value: T) -> Self {
        let test_thread_id = thread::current().id();
        debug!("FakeSendSync::new() with strong_count=1 on thread {:?}", test_thread_id);
        Self {
            test_thread_id,
            other_thread_id: None,
            value: Arc::new(value),
        }
    }
}

impl<T> Deref for FakeSendSync<T> {
    type Target = T;

    #[track_caller]
    fn deref(&self) -> &Self::Target {
        // FIXME
        debug!("FakeSendSync::deref() with strong_count={} on thread {:?}", Arc::strong_count(&self.value), thread::current().id());
        // TODO: Switch back to assert_eq!
        if self.test_thread_id != thread::current().id() {
            error!("Current thread {:?} does not match test_thread_id={:?}", thread::current().id(), self.test_thread_id);
        }
        &self.value
    }
}

impl<T> Clone for FakeSendSync<T> {
    fn clone(&self) -> Self {
        let value = self.value.clone();
        debug!("FakeSendSync::clone() with new strong_count={} on thread {:?}", Arc::strong_count(&self.value), thread::current().id());

        Self {
            test_thread_id: self.test_thread_id,
            other_thread_id: self.other_thread_id,
            value,
        }
    }
}

impl<T> Drop for FakeSendSync<T> {
    fn drop(&mut self) {
        debug!("FakeSendSync::drop() with new strong_count={} on thread {:?}", Arc::strong_count(&self.value) - 1, thread::current().id());
    }
}