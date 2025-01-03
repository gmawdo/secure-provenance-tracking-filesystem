use std::sync::Arc;
use graymamba::kernel::protocol::tcp::{NFSTcp, NFSTcpListener};
use graymamba::sharesfs::SharesFS;

use graymamba::audit_adapters::irrefutable_audit::IrrefutableAudit;
#[cfg(feature = "merkle_audit")]
use graymamba::audit_adapters::merkle_audit::MerkleBasedAuditSystem;

#[cfg(feature = "az_audit")]
use graymamba::audit_adapters::substrate_audit::SubstrateAuditSystem;

use config::{Config, File as ConfigFile};

use tokio::signal;
use std::io::Write;
use tracing_subscriber::EnvFilter;

const HOSTPORT: u32 = 2049;

#[tokio::main]
async fn main() {
    // Load settings but skip logging config since we've already set it up
    let mut settings = Config::default();
    settings
        .merge(ConfigFile::with_name("config/settings.toml"))
        .expect("Failed to load configuration");

    // Retrieve log settings from configuration
    let base_level = settings
        .get::<String>("logging.level")
        .unwrap_or_else(|_| "warn".to_string());

    // Build the filter with both base level and all module directives
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            let mut filter = EnvFilter::new(&base_level);
            if let Ok(filters) = settings.get::<Vec<String>>("logging.module_filter") {
                for module_filter in filters {
                    filter = filter.add_directive(module_filter.parse().unwrap());
                }
            }
            filter
        });
    println!("filter: {:?}", filter);

    // Single initialization with combined settings
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)  // Don't show target module
        .with_thread_ids(false)  // Don't show thread IDs
        .with_thread_names(false)  // Don't show thread names
        .with_file(true)  // Don't show file names
        .with_line_number(true)  // Don't show line numbers
        .with_level(true)  // Do show log levels
        .compact()  // Use compact formatting
        .init();

    let version = env!("CARGO_PKG_VERSION");
    println!("Application version: {}", version);

    // Print enabled features
    println!("Enabled features:");

    SharesFS::set_namespace_id_and_community(settings.get_str("storage.namespace_id").unwrap().as_str(), settings.get_str("storage.community").unwrap().as_str()).await;

    let data_store = {
        #[cfg(feature = "redis_store")]
        {
            use graymamba::backingstore::redis_data_store::RedisDataStore;
            Arc::new(RedisDataStore::new()
                .expect("Failed to create Redis data store"))
        }

        #[cfg(feature = "rocksdb_store")]
        {
            use graymamba::backingstore::rocksdb_data_store::RocksDBDataStore;
            Arc::new(RocksDBDataStore::new(
                settings.get_str("storage.rocksdb_path")
                    .expect("Failed to get rocksdb_path from settings")
                    .as_str()
            ).expect("Failed to create RocksDB data store"))
        }

        #[cfg(not(any(feature = "redis_store", feature = "rocksdb_store")))]
        compile_error!("Either 'redis_store' or 'rocksdb_store' feature must be enabled");
    };
    

    let audit_system: Arc<dyn IrrefutableAudit> = {
        #[cfg(feature = "merkle_audit")]
        {
            match MerkleBasedAuditSystem::new().await {
                Ok(audit) => {
                    println!("✅ Merkle-based audit initialization successful");
                    Arc::new(audit)
                },
                Err(e) => {
                    eprintln!("❌ Fatal Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        
        #[cfg(feature = "az_audit")]
        {
            match SubstrateAuditSystem::new().await {
                Ok(audit) => {
                    println!("✅ Aleph Zero audit initialization successful");
                    Arc::new(audit)
                },
                Err(e) => {
                    eprintln!("❌ Fatal Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        
        #[cfg(not(any(feature = "merkle_audit", feature = "az_audit")))]
        compile_error!("Either 'merkle_audit' or 'az_audit' feature must be enabled");
    };

    let shares_fs = SharesFS::new(data_store, audit_system.clone());
    let shares_fs_clone = shares_fs.clone();
    tokio::spawn(async move {
        shares_fs_clone.start_monitoring().await;
    });

    println!("🚀 graymamba launched");
    let listener = NFSTcpListener::bind(&format!("0.0.0.0:{HOSTPORT}"), shares_fs)
        .await
        .unwrap();
    // Start the listener in a separate task
    let _listener_handle = tokio::spawn(async move {
        listener.handle_forever().await
    });

    // Wait for ctrl+c
    match signal::ctrl_c().await {
        Ok(()) => {
            println!("Received shutdown signal");
            std::io::stdout().flush().unwrap();  // Ensure output is displayed
        }
        Err(err) => {
            eprintln!("Error handling ctrl-c: {}", err);
            std::io::stderr().flush().unwrap();
        }
    }

    // Perform cleanup
    std::io::stdout().flush().unwrap();
    audit_system.shutdown().unwrap();
}

