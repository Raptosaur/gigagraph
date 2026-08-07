//! DI-style wiring: trait -> impl, typed struct fields (plain, reference,
//! Arc/Box<dyn Trait>), typed params, and constructed locals.

use std::sync::Arc;

pub struct ConnectionPool;

impl ConnectionPool {
    pub fn write(&self, _key: &str) -> bool {
        true
    }
}

pub struct Config;

pub trait Store {
    fn save(&self, key: &str) -> bool;
}

pub struct DbStore {
    pool: ConnectionPool,
}

impl DbStore {
    pub fn new() -> Self {
        DbStore {
            pool: ConnectionPool,
        }
    }
}

impl Store for DbStore {
    fn save(&self, key: &str) -> bool {
        self.pool.write(key)
    }
}

pub struct Service {
    store: DbStore,
    shared: Arc<dyn Store>,
    fallback: Box<dyn Store>,
    cache: Rc<Cache>,
    config: &'static Config,
}

impl Service {
    pub fn with_store(store: DbStore, shared: Arc<dyn Store>, config: &Config) -> Self {
        Service {
            store,
            shared: shared.clone(),
            fallback: Box::new(DbStore::new()),
            cache: Rc::new(Cache),
            config: leak(config),
        }
    }

    pub fn run(&self) {
        let s = DbStore::new();
        let annotated: DbStore = make_store();
        let borrowed: &Config = default_config();
        let wrapped = Arc::new(DbStore::new());
        self.store.save("run");
        s.save("local");
        let _ = (annotated, borrowed, wrapped);
    }
}
