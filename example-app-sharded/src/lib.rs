#[allow(dead_code)]
pub mod generated;

use generated::ShardStrategy;

/// A trivial modulo strategy: map a UUID onto `0..shard_count` by its low bits.
///
/// Real deployments would use consistent hashing or a lookup table; the shape is
/// the same. `shard_index` is async so a strategy may consult a cache or catalog
/// database.
#[derive(Clone, Copy, Default)]
pub struct ModuloStrategy;

impl ShardStrategy<uuid::Uuid> for ModuloStrategy {
    async fn shard_index(&self, key: &uuid::Uuid, shard_count: usize) -> usize {
        (key.as_u128() % shard_count as u128) as usize
    }
}

/// A router over `shard_count` pools, all backed by `database_url`.
///
/// A single physical database standing in for every logical shard is enough to
/// exercise the generated routing, transaction-pinning and batch-consistency
/// code without provisioning multiple servers.
pub type AccountsRouter = generated::PoolRouter<uuid::Uuid, ModuloStrategy>;

pub async fn build_router(
    database_url: &str,
    shard_count: usize,
) -> Result<AccountsRouter, sqlx::Error> {
    let mut pools = Vec::with_capacity(shard_count);
    for _ in 0..shard_count {
        pools.push(sqlx::PgPool::connect(database_url).await?);
    }
    Ok(generated::PoolRouter::new(pools, ModuloStrategy))
}
