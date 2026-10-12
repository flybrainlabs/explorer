//! Dedicated query-only process. It opens only the public projection, never
//! target-v2.sqlite, signing keys, private artifacts or the reward writer.
use std::{env,path::PathBuf};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<_>=env::args().collect();if args.len()!=3{return Err("usage: flypow_explorer_query PUBLIC_INDEX LOOPBACK_ADDRESS".into());}
 let index=PathBuf::from(&args[1]);if !index.is_file(){return Err("public index required; build it before starting queries".into());}
 let address=args[2].clone();
 let runtime=tokio::runtime::Builder::new_multi_thread().worker_threads(2).max_blocking_threads(16).enable_all().build()?;
 runtime.block_on(async move{let listener=tokio::net::TcpListener::bind(&address).await?;axum::serve(listener,flypow::explorer::standalone_routes(index)).await})?;Ok(())
}
