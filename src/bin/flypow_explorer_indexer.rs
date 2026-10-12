//! Read-only source replay into an independent query database. Never opens an
//! Authority for mutation, creates an identity, dispatches work or emits rewards.
use std::{env,path::PathBuf};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<_>=env::args().collect();if args.len()<6{return Err("usage: flypow_explorer_indexer DATA INDEX NETWORK AUTHORITY CAMPAIGN [--rebuild]".into());}
 let data=PathBuf::from(&args[1]);let index=PathBuf::from(&args[2]);
 let report=if args.iter().any(|a|a=="--rebuild"){flypow::explorer_projection::rebuild(&data,&index,&args[3],&args[4],&args[5])}else{flypow::explorer_projection::project(&data,&index,&args[3],&args[4],&args[5])}?;
 println!("{}",serde_json::to_string(&report)?);Ok(())
}
