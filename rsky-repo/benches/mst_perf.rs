//! MST performance benchmarks over both an in-memory blockstore and a real
//! on-disk SQLite blockstore modelled on `rsky_pds::actor_store::repo::SqlRepoReader`.
//!
//! Run with: `cargo bench -p rsky-repo`
//! Restrict with: `cargo bench -p rsky-repo -- covering_proof`

use anyhow::Result;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use lexicon_cid::Cid;
use rsky_common::ipld::cid_for_cbor;
use rsky_repo::block_map::{BlockMap, BlocksAndMissing};
use rsky_repo::cid_set::CidSet;
use rsky_repo::mst::MST;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_repo::storage::readable_blockstore::ReadableBlockstore;
use rsky_repo::storage::types::RepoStorage;
use rsky_repo::types::CommitData;
use rusqlite::{Connection, OptionalExtension};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

const SIZES: [usize; 3] = [1_000, 10_000, 50_000];
const BATCH_OPS: usize = 200;

// ---------------------------------------------------------------------------
// SQLite blockstore
// ---------------------------------------------------------------------------

/// Minimal `RepoStorage` over a real SQLite file. Schema, pragmas, per-instance
/// block cache and chunked `IN (...)` reads mirror the PDS `SqlRepoReader` so the
/// numbers are representative of a live actor store.
#[derive(Clone, Debug)]
pub struct SqliteBlockstore {
    cache: Arc<RwLock<BlockMap>>,
    conn: Arc<Mutex<Connection>>,
    root: Arc<RwLock<Option<Cid>>>,
}

fn placeholders(len: usize) -> String {
    vec!["?"; len].join(",")
}

impl SqliteBlockstore {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS repo_block (\
                cid TEXT PRIMARY KEY, \
                \"repoRev\" TEXT NOT NULL, \
                size INTEGER NOT NULL, \
                content BLOB NOT NULL\
            );\
            CREATE INDEX IF NOT EXISTS repo_block_repo_rev_idx ON repo_block (\"repoRev\", cid);",
        )?;
        Ok(Self {
            cache: Arc::new(RwLock::new(BlockMap::new())),
            conn: Arc::new(Mutex::new(conn)),
            root: Arc::new(RwLock::new(None)),
        })
    }

    /// A handle onto the same file with an empty block cache, i.e. what a PDS
    /// builds per request.
    pub fn fresh(&self) -> Self {
        Self {
            cache: Arc::new(RwLock::new(BlockMap::new())),
            conn: self.conn.clone(),
            root: self.root.clone(),
        }
    }

    async fn run<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().expect("sqlite mutex poisoned");
            f(&conn)
        })
        .await?
    }
}

impl ReadableBlockstore for SqliteBlockstore {
    fn get_bytes<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>>> + Send + Sync + 'a>> {
        let cid = *cid;
        Box::pin(async move {
            let cached = {
                let guard = self.cache.read().await;
                guard.get(cid).cloned()
            };
            if let Some(hit) = cached {
                return Ok(Some(hit));
            }
            let found: Option<Vec<u8>> = self
                .run(move |conn| {
                    Ok(conn
                        .query_row(
                            "SELECT content FROM repo_block WHERE cid = ?1",
                            [cid.to_string()],
                            |row| row.get(0),
                        )
                        .optional()?)
                })
                .await?;
            if let Some(ref bytes) = found {
                let mut guard = self.cache.write().await;
                guard.set(cid, bytes.clone());
            }
            Ok(found)
        })
    }

    fn has<'a>(
        &'a self,
        cid: Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + Sync + 'a>> {
        Box::pin(async move { Ok(self.get_bytes(&cid).await?.is_some()) })
    }

    fn get_blocks<'a>(
        &'a self,
        cids: Vec<Cid>,
    ) -> Pin<Box<dyn Future<Output = Result<BlocksAndMissing>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let cached = {
                let mut guard = self.cache.write().await;
                guard.get_many(cids)?
            };
            if cached.missing.is_empty() {
                return Ok(cached);
            }
            let missing_strings: Vec<String> =
                cached.missing.iter().map(|c| c.to_string()).collect();
            let mut missing = CidSet::new(Some(cached.missing));
            let rows: Vec<(String, Vec<u8>)> = self
                .run(move |conn| {
                    let mut rows = Vec::new();
                    for batch in missing_strings.chunks(500) {
                        let sql = format!(
                            "SELECT cid, content FROM repo_block WHERE cid IN ({})",
                            placeholders(batch.len())
                        );
                        let mut stmt = conn.prepare(&sql)?;
                        let batch_rows = stmt
                            .query_map(rusqlite::params_from_iter(batch.iter()), |row| {
                                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                            })?
                            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
                        rows.extend(batch_rows);
                    }
                    Ok(rows)
                })
                .await?;
            let mut blocks = BlockMap::new();
            for (cid_str, content) in rows {
                let cid = Cid::from_str(&cid_str)?;
                blocks.set(cid, content);
                missing.delete(cid);
            }
            {
                let mut guard = self.cache.write().await;
                guard.add_map(blocks.clone())?;
            }
            blocks.add_map(cached.blocks)?;
            Ok(BlocksAndMissing {
                blocks,
                missing: missing.to_list(),
            })
        })
    }
}

impl RepoStorage for SqliteBlockstore {
    fn get_root<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<Cid>> + Send + Sync + 'a>> {
        Box::pin(async move { *self.root.read().await })
    }

    fn put_block<'a>(
        &'a self,
        cid: Cid,
        bytes: Vec<u8>,
        rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let mut map = BlockMap::new();
            map.set(cid, bytes);
            self.put_many(map, rev).await
        })
    }

    fn put_many<'a>(
        &'a self,
        to_put: BlockMap,
        rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let entries: Vec<(String, usize, Vec<u8>)> = to_put
                .entries()?
                .into_iter()
                .map(|e| (e.cid.to_string(), e.bytes.len(), e.bytes))
                .collect();
            self.run(move |conn| {
                conn.execute_batch("BEGIN IMMEDIATE")?;
                {
                    let mut stmt = conn.prepare(
                        "INSERT OR IGNORE INTO repo_block (cid, \"repoRev\", size, content) \
                         VALUES (?1, ?2, ?3, ?4)",
                    )?;
                    for (cid, size, content) in entries {
                        stmt.execute(rusqlite::params![cid, rev, size as i64, content])?;
                    }
                }
                conn.execute_batch("COMMIT")?;
                Ok(())
            })
            .await
        })
    }

    fn update_root<'a>(
        &'a self,
        cid: Cid,
        _rev: String,
        _is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            *self.root.write().await = Some(cid);
            Ok(())
        })
    }

    fn apply_commit<'a>(
        &'a self,
        commit: CommitData,
        _is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let rev = commit.rev.clone();
            self.put_many(commit.new_blocks, rev).await?;
            *self.root.write().await = Some(commit.cid);
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    Memory,
    Sqlite,
}

impl Backend {
    fn name(self) -> &'static str {
        match self {
            Backend::Memory => "memory",
            Backend::Sqlite => "sqlite",
        }
    }
}

fn record_key(i: usize) -> String {
    format!("app.bsky.feed.post/{:0>13}", i)
}

fn record_value(i: usize) -> Cid {
    cid_for_cbor(&json!({"$type": "app.bsky.feed.post", "text": format!("post {i}")})).unwrap()
}

struct Fixture {
    backend: Backend,
    size: usize,
    memory: Option<MemoryBlockstore>,
    sqlite: Option<SqliteBlockstore>,
    _dir: Option<tempfile::TempDir>,
    root: Cid,
}

impl Fixture {
    /// A storage handle with a cold per-request cache, as a PDS builds per write.
    fn storage(&self) -> Arc<RwLock<dyn RepoStorage>> {
        match self.backend {
            Backend::Memory => Arc::new(RwLock::new(self.memory.clone().unwrap())),
            Backend::Sqlite => Arc::new(RwLock::new(self.sqlite.as_ref().unwrap().fresh())),
        }
    }

    /// A virtual (unhydrated) tree rooted at the fixture root.
    fn cold_tree(&self) -> MST {
        MST::load(self.storage(), self.root, None).unwrap()
    }

    async fn warm_tree(&self) -> MST {
        let mut mst = self.cold_tree();
        // Touch every node once so entries are cached, isolating in-memory cost.
        let _ = mst.get_covering_proof(&record_key(self.size / 2)).await;
        let _ = mst.get_covering_proof(&record_key(0)).await;
        let _ = mst.get_covering_proof(&record_key(self.size - 1)).await;
        mst
    }

    fn probe_key(&self) -> String {
        record_key(self.size / 2)
    }

    fn new_key(&self, n: usize) -> String {
        record_key(self.size + 1 + n)
    }
}

async fn build_fixture(backend: Backend, size: usize) -> Result<Fixture> {
    let (memory, sqlite, dir) = match backend {
        Backend::Memory => (Some(MemoryBlockstore::default()), None, None),
        Backend::Sqlite => {
            let dir = tempfile::tempdir()?;
            let store = SqliteBlockstore::open(&dir.path().join("actor.sqlite"))?;
            (None, Some(store), Some(dir))
        }
    };

    let storage: Arc<RwLock<dyn RepoStorage>> = match backend {
        Backend::Memory => Arc::new(RwLock::new(memory.clone().unwrap())),
        Backend::Sqlite => Arc::new(RwLock::new(sqlite.clone().unwrap())),
    };

    // Persist the record blocks in one batch.
    let mut leaves = BlockMap::new();
    let mut pairs: Vec<(String, Cid)> = Vec::with_capacity(size);
    for i in 0..size + BATCH_OPS + 8 {
        let rec = json!({"$type": "app.bsky.feed.post", "text": format!("post {i}")});
        let cid = cid_for_cbor(&rec)?;
        leaves.set(cid, rsky_common::struct_to_cbor(&rec)?);
        if i < size {
            pairs.push((record_key(i), cid));
        }
    }
    {
        let guard = storage.read().await;
        guard.put_many(leaves, "bench".to_string()).await?;
    }

    let mut mst = MST::create(storage.clone(), None, None).await?;
    for (key, value) in &pairs {
        mst = mst.add(key, *value, None).await?;
    }
    let root = mst.save_mst().await?;

    Ok(Fixture {
        backend,
        size,
        memory,
        sqlite,
        _dir: dir,
        root,
    })
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn fixtures(rt: &Runtime) -> Vec<Fixture> {
    let mut out = Vec::new();
    for backend in [Backend::Memory, Backend::Sqlite] {
        for size in SIZES {
            out.push(rt.block_on(build_fixture(backend, size)).unwrap());
        }
    }
    out
}

fn bench_all(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let fixtures = fixtures(&rt);

    // get_covering_proof against a hydrated tree: pure in-process cost.
    {
        let mut group = c.benchmark_group("covering_proof_warm");
        for f in &fixtures {
            let tree = rt.block_on(f.warm_tree());
            let key = f.probe_key();
            group.bench_with_input(
                BenchmarkId::new(f.backend.name(), f.size),
                &f.size,
                |b, _| {
                    b.iter(|| {
                        rt.block_on(async {
                            let mut tree = tree.clone();
                            tree.get_covering_proof(&key).await.unwrap()
                        })
                    })
                },
            );
        }
        group.finish();
    }

    // get_covering_proof from a cold repo load: what a single PDS write pays.
    {
        let mut group = c.benchmark_group("covering_proof_cold");
        group.sample_size(40);
        for f in &fixtures {
            let key = f.probe_key();
            group.bench_with_input(
                BenchmarkId::new(f.backend.name(), f.size),
                &f.size,
                |b, _| {
                    b.iter_batched(
                        || f.cold_tree(),
                        |mut tree| {
                            rt.block_on(async { tree.get_covering_proof(&key).await.unwrap() })
                        },
                        BatchSize::SmallInput,
                    )
                },
            );
        }
        group.finish();
    }

    {
        let mut group = c.benchmark_group("add");
        for f in &fixtures {
            let tree = rt.block_on(f.warm_tree());
            let key = f.new_key(0);
            let value = record_value(f.size + 1);
            group.bench_with_input(
                BenchmarkId::new(f.backend.name(), f.size),
                &f.size,
                |b, _| {
                    b.iter(|| {
                        rt.block_on(async {
                            let mut tree = tree.clone();
                            tree.add(&key, value, None).await.unwrap()
                        })
                    })
                },
            );
        }
        group.finish();
    }

    {
        let mut group = c.benchmark_group("delete");
        for f in &fixtures {
            let tree = rt.block_on(f.warm_tree());
            let key = f.probe_key();
            group.bench_with_input(
                BenchmarkId::new(f.backend.name(), f.size),
                &f.size,
                |b, _| {
                    b.iter(|| {
                        rt.block_on(async {
                            let mut tree = tree.clone();
                            tree.delete(&key).await.unwrap()
                        })
                    })
                },
            );
        }
        group.finish();
    }

    // applyWrites shape: N creates applied to the tree, then one covering proof
    // per write against the final tree (see Repo::format_commit).
    {
        let mut group = c.benchmark_group("apply_writes_200");
        group.sample_size(10);
        for f in &fixtures {
            let keys: Vec<String> = (0..BATCH_OPS).map(|n| f.new_key(n)).collect();
            let values: Vec<Cid> = (0..BATCH_OPS)
                .map(|n| record_value(f.size + 1 + n))
                .collect();
            group.bench_with_input(
                BenchmarkId::new(f.backend.name(), f.size),
                &f.size,
                |b, _| {
                    b.iter_batched(
                        || f.cold_tree(),
                        |mut tree| {
                            rt.block_on(async {
                                for (key, value) in keys.iter().zip(values.iter()) {
                                    tree = tree.add(key, *value, None).await.unwrap();
                                }
                                let mut proof = BlockMap::new();
                                for key in keys.iter() {
                                    proof
                                        .add_map(tree.get_covering_proof(key).await.unwrap())
                                        .unwrap();
                                }
                                proof
                            })
                        },
                        BatchSize::SmallInput,
                    )
                },
            );
        }
        group.finish();
    }
}

criterion_group!(benches, bench_all);
criterion_main!(benches);
