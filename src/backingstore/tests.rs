use crate::backingstore::data_store::DataStore;
use crate::backingstore::redis_data_store::RedisDataStore;
use crate::backingstore::rocksdb_data_store::RocksDBDataStore;
use tempfile::tempdir;
use graymamba::sharesfs::SharesFS;

const TEST_COMMUNITY: &str = "orangery";
const TEST_NAMESPACE_ID: &str = "citrus";
async fn setup_redis() -> RedisDataStore {
    RedisDataStore::new().expect("Failed to create Redis store")
}

async fn setup_rocksdb() -> RocksDBDataStore {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    RocksDBDataStore::new(temp_dir.path().to_str().unwrap()).expect("Failed to create RocksDB store")
}

fn check_redis_running() -> bool {
    // Attempt to connect to Redis and return true if successful
    let client = redis::Client::open("redis://127.0.0.1:6380"); //one of our expected cluster nodes
    match client {
        Ok(c) => {
            let con = c.get_connection();
            match con {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

#[tokio::test]
async fn test_init_user_directory_structure() {
    let rocks = setup_rocksdb().await;
    let stores: Vec<(&str, &dyn DataStore)> = vec![/*("redis", &redis),*/ ("rocks", &rocks)];

    for (name, store) in stores {
        SharesFS::set_namespace_id_and_community(TEST_NAMESPACE_ID, TEST_COMMUNITY).await;
        // Test 1: Initialize root directory
        store.init_user_directory("/").await.expect(&format!("{} root init failed", name));

        // Test 2: Verify root directory structure
        let root_key = &format!("{{{}}}:/", TEST_COMMUNITY);
        let root_metadata = store.hgetall(root_key).await
            .expect(&format!("{} failed to get root metadata", name));
        
        assert!(root_metadata.iter().any(|(k, _)| k == "fileid"), 
            "{} root missing fileid", name);
        assert!(root_metadata.iter().any(|(k, _)| k == "ftype"), 
            "{} root missing ftype", name);

        // Test 3: Check nodes structure
        let nodes_key = &format!("{{{}}}:/{}_nodes", TEST_COMMUNITY, TEST_NAMESPACE_ID);
        let nodes = store.zrange_withscores(nodes_key, 0, -1).await
            .expect(&format!("{} failed to get nodes", name));
        
        assert!(!nodes.is_empty(), "{} nodes should not be empty", name);
        assert_eq!(nodes[0].1, 1.0, "{} root score should be 1.0", name);

        // Test 4: Initialize subdirectory
        store.init_user_directory("/test").await
            .expect(&format!("{} subdir init failed", name));

        // Test 5: Verify subdirectory structure
        let subdir_key =  &format!("{{{}}}:/test", TEST_COMMUNITY);
        let subdir_metadata = store.hgetall(subdir_key).await
            .expect(&format!("{} failed to get subdir metadata", name));
        
        assert!(subdir_metadata.iter().any(|(k, _)| k == "fileid"), 
            "{} subdir missing fileid", name);

        // Test 6: Check path_to_id mapping
        let path_to_id_key = &format!("{{{}}}:/{}_path_to_id", TEST_COMMUNITY, TEST_NAMESPACE_ID);
        let path_id = store.hget(path_to_id_key, "/test").await
            .expect(&format!("{} failed to get path_to_id", name));
        
        assert!(!path_id.is_empty(), "{} path_to_id should exist", name);

        // Test 7: Check id_to_path mapping
        let id_to_path_key = &format!("{{{}}}:/{}_id_to_path", TEST_COMMUNITY, TEST_NAMESPACE_ID);
        let id = store.hget(path_to_id_key, "/test").await
            .expect(&format!("{} failed to get id", name));
        let path = store.hget(id_to_path_key, &id).await
            .expect(&format!("{} failed to get id_to_path", name));
        
        assert_eq!(path, "/test", "{} path mismatch in id_to_path", name);

        // Test 8: Verify next_fileid
        let next_fileid_key = &format!("{{{}}}:/{}_next_fileid", TEST_COMMUNITY, TEST_NAMESPACE_ID);
        let next_fileid = store.get(next_fileid_key).await
            .expect(&format!("{} failed to get next_fileid", name));
        
        assert!(!next_fileid.is_empty(), "{} next_fileid should exist", name);
    }
}

#[tokio::test]
async fn test_directory_idempotency() {

    if !check_redis_running() {
        println!("Skipping test_redis_functionality: Redis server is not running.");
        return; // Skip the test
    }

    let redis = setup_redis().await;
    let rocks = setup_rocksdb().await;
    let stores: Vec<(&str, &dyn DataStore)> = vec![("redis", &redis), ("rocks", &rocks)];

    for (name, store) in stores {
        // Test 1: Initialize directory twice
        store.init_user_directory("/test2").await
            .expect(&format!("{} first init failed", name));
        let first_id = store.hget(&format!("{{{}}}:/test2", TEST_COMMUNITY), "fileid").await
            .expect(&format!("{} failed to get first fileid", name));
        
        store.init_user_directory("/test2").await
            .expect(&format!("{} second init failed", name));
        let second_id = store.hget(&format!("{{{}}}:/test2", TEST_COMMUNITY), "fileid").await
            .expect(&format!("{} failed to get second fileid", name));
        
        assert_eq!(first_id, second_id, 
            "{} fileid changed on second init", name);
    }
}

#[tokio::test]
async fn test_sorted_set_operations() {
    if !check_redis_running() {
        println!("Skipping test_redis_functionality: Redis server is not running.");
        return; // Skip the test
    }

    let redis = setup_redis().await;
    let rocks = setup_rocksdb().await;
    let stores: Vec<(&str, &dyn DataStore)> = vec![("redis", &redis), ("rocks", &rocks)];

    for (name, store) in stores {
        // Test 1: Add items to sorted set
        let key = "test_sorted_set";
        store.zadd(key, "item1", 1.0).await
            .expect(&format!("{} zadd failed", name));
        store.zadd(key, "item2", 2.0).await
            .expect(&format!("{} zadd failed", name));

        // Test 2: Get range with scores
        let items = store.zrange_withscores(key, 0, -1).await
            .expect(&format!("{} zrange failed", name));
        
        assert_eq!(items.len(), 2, "{} wrong number of items", name);
        assert_eq!(items[0].0, "item1", "{} wrong item order", name);
        assert_eq!(items[0].1, 1.0, "{} wrong score", name);
    }
} 