#![recursion_limit = "256"]
#![allow(dead_code, unused_imports)]

#[path = "../bootstrap.rs"]
mod bootstrap;
#[path = "../bootstrap_compact.rs"]
mod bootstrap_compact;
#[path = "../compatibility.rs"]
mod compatibility;
#[path = "../config.rs"]
mod config;
#[path = "../edge_cache.rs"]
mod edge_cache;
#[path = "../nats.rs"]
mod nats;
#[path = "../observability_policy.rs"]
mod observability_policy;
#[path = "../postgres.rs"]
mod postgres;
#[path = "../profiles.rs"]
mod profiles;
#[path = "../qdrant.rs"]
mod qdrant;
#[path = "../s3.rs"]
mod s3;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args = bootstrap_compact::parse_args(std::env::args().skip(1).collect::<Vec<_>>())?;
    bootstrap_compact::run(args).await
}
