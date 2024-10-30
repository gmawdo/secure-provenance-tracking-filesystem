use std::sync::Arc;
use r2d2_redis_cluster::Commands;
use async_trait::async_trait;
use crate::data_store::{DataStore, DataStoreError};
use config::{Config, File as ConfigFile, ConfigError};

use r2d2_redis_cluster::{r2d2, RedisClusterConnectionManager};

pub struct RedisClusterPool {
    pub pool: r2d2::Pool<RedisClusterConnectionManager>,
}

impl RedisClusterPool {
    pub fn new(redis_urls: Vec<&str>, max_size: u32) -> Result<RedisClusterPool, Box<dyn std::error::Error>> {
        let manager = RedisClusterConnectionManager::new(redis_urls.clone())
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let pool = r2d2::Pool::builder()
            .max_size(max_size)
            .build(manager)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        
        Ok(RedisClusterPool { pool })
    }

    pub fn from_config_file() -> Result<RedisClusterPool, ConfigError> {
        let mut settings = Config::default();
        settings.merge(ConfigFile::with_name("config/settings.toml"))?;
        
        let redis_nodes: Vec<String> = settings.get::<Vec<String>>("cluster_nodes")?;
        let redis_nodes: Vec<&str> = redis_nodes.iter().map(|s| s.as_str()).collect();
        
        let max_size: u32 = settings.get("redis_pool_max_size").unwrap_or(1000);

        RedisClusterPool::new(redis_nodes, max_size).map_err(|e| {
            ConfigError::Message(e.to_string())
        })
    }
}

pub struct RedisDataStore {
    pool: Arc<r2d2::Pool<r2d2_redis_cluster::RedisClusterConnectionManager>>,
}

impl RedisDataStore {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let redis_pool = RedisClusterPool::from_config_file()?;
        let inner_pool = Arc::new(redis_pool.pool);
        Ok(RedisDataStore { pool: inner_pool })
    }
}

#[async_trait]
impl DataStore for RedisDataStore {
    async fn get(&self, key: &str) -> Result<String, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.get(key).map_err(|_| DataStoreError::KeyNotFound)
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.set(key, value).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn delete(&self, key: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.del(key).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn hget(&self, key: &str, field: &str) -> Result<String, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.hget(key, field).map_err(|_| DataStoreError::KeyNotFound)
    }

    async fn hset(&self, key: &str, field: &str, value: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.hset(key, field, value).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn hdel(&self, key: &str, field: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.hdel(key, field).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn hgetall(&self, key: &str) -> Result<Vec<(String, String)>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.hgetall(key).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn incr(&self, key: &str) -> Result<i64, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.incr(key, 1).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn rename(&self, old_key: &str, new_key: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.rename(old_key, new_key).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.keys(pattern).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn zrange_withscores(&self, key: &str, start: isize, stop: isize) -> Result<Vec<(String, f64)>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zrange_withscores(key, start, stop).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn zadd(&self, key: &str, member: &str, score: f64) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zadd(key, member, score).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn zrem(&self, key: &str, member: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zrem(key, member).map_err(|_| DataStoreError::OperationFailed)
    }
    async fn zrangebyscore(&self, key: &str, min: f64, max: f64) -> Result<Vec<String>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zrangebyscore(key, min, max).map_err(|_| DataStoreError::OperationFailed)
    }
    async fn hset_multiple(&self, key: &str, fields: &[(&str, &str)]) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.hset_multiple::<_, _, _, ()>(key, fields).map_err(|_| DataStoreError::OperationFailed)?;
        Ok(())
    }
    async fn zscan_match(&self, key: &str, pattern: &str) -> Result<Vec<String>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        let results: Vec<(String, f64)> = conn.zscan_match(key, pattern)
            .map_err(|_| DataStoreError::OperationFailed)?
            .collect();
        Ok(results.into_iter().map(|(member, _)| member).collect())
    }

    async fn zscore(&self, key: &str, member: &str) -> Result<Option<f64>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zscore(key, member).map_err(|_| DataStoreError::OperationFailed)
    }
}