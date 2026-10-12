//! Read-only Explorer queries over a separate public SQLite projection.
use crate::{target_authority::{Authority,now},explorer_types::*,explorer_projection};
use axum::{Router,routing::get,extract::{State,Path,Query},Json,http::{StatusCode,HeaderMap},response::{Response,IntoResponse,sse::{Sse,Event,KeepAlive}}};
use rusqlite::{Connection,params,OptionalExtension};
use serde_json::{Value,json};
use flypow_target_protocol::{self as p,Result};
use std::sync::Arc;
fn failure(e:rusqlite::Error)->String{format!("INDEX_UNAVAILABLE: {e}")}
fn json_value(s:String)->Result<Value>{serde_json::from_str(&s).map_err(|_|"INDEX_UNAVAILABLE: document".into())}
fn meta(c:&Connection)->Result<Value>{
    let asof:u64=c.query_row("SELECT value FROM explorer_meta WHERE key='asOf'",[],|r|r.get::<_,String>(0)).map_err(failure)?.parse().map_err(|_|"index time")?;
    let mut q=c.prepare("SELECT source,sequence FROM explorer_checkpoints ORDER BY source").map_err(failure)?;let mut sources=serde_json::Map::new();
    for row in q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?))).map_err(failure)?{let(k,n)=row.map_err(failure)?;sources.insert(k,json!(n.to_string()));}
    Ok(json!({"indexVersion":INDEX_VERSION,"asOf":asof,"freshness":if now().saturating_sub(asof)>30{"stale"}else{"fresh"},"snapshotCursor":c.query_row("SELECT COALESCE(MAX(sequence),0) FROM explorer_events",[],|r|r.get::<_,i64>(0)).map_err(failure)?.to_string(),"indexedThrough":sources,"source":"authority transaction outbox and explicitly authorized publications","scope":"project records, not all Solana blocks","readOnly":true}))
}
fn wrapped(c:&Connection,data:Value)->Result<Value>{let m=meta(c)?;Ok(json!({"data":data,"snapshot":m,"source":m["source"],"indexVersion":INDEX_VERSION,"asOf":m["asOf"],"freshness":m["freshness"],"scope":m["scope"]}))}
pub fn query_index(c:&Connection,path:&str,q:&ExplorerQuery)->Result<Value>{
    if path.len()>700||q.q.len()>256||[&q.goal_id,&q.miner_id,&q.model_root,&q.campaign_id,&q.namespace].iter().any(|s|s.len()>128){return Err("INVALID_INPUT: query bounds".into());}
    match path {
        "overview"=>{let mut v=json_value(c.query_row("SELECT value FROM explorer_meta WHERE key='overview'",[],|r|r.get(0)).map_err(failure)?)?;v["latestRecords"]=list(c,"record",&ExplorerQuery{limit:5,..Default::default()})?["items"].clone();v["latestTransactions"]=list(c,"transaction",&ExplorerQuery{limit:5,..Default::default()})?["items"].clone();wrapped(c,v)},
        "network"=>{let v=json_value(c.query_row("SELECT value FROM explorer_meta WHERE key='overview'",[],|r|r.get(0)).map_err(failure)?)?;wrapped(c,json!({"network":v["network"],"supply":v["supply"],"metrics":v["metrics"],"chainStatus":v["chainStatus"],"chainNamespaces":v["chainNamespaces"]}))},
        "records"=>list(c,"record",q),"rounds"=>list(c,"round",q),"search"=>search(c,q),
        "events"=>{let mut s=c.prepare("SELECT sequence,payload FROM explorer_events WHERE sequence>? ORDER BY sequence LIMIT 128").map_err(failure)?;let rows=s.query_map([q.after as i64],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).map_err(failure)?.collect::<rusqlite::Result<Vec<_>>>().map_err(failure)?;let items=rows.iter().map(|(n,s)|Ok(json!({"cursor":n.to_string(),"event":json_value(s.clone())?}))).collect::<Result<Vec<_>>>()?;let mut v=wrapped(c,json!(items))?;v["items"]=json!(items);v["nextCursor"]=json!(rows.last().map(|r|r.0.to_string()).unwrap_or(q.after.to_string()));v["hasMore"]=json!(rows.len()==128);Ok(v)},
        _=>detail(c,path,q),
    }
}
fn list(c:&Connection,kind:&str,q:&ExplorerQuery)->Result<Value>{
    let limit=q.limit.clamp(1,50).max(if q.limit==0{20}else{1});let filter=filter_key(kind,q)?;
    let cursor=if q.cursor.is_empty(){Cursor{version:INDEX_VERSION.into(),filter:filter.clone(),snapshot:c.query_row("SELECT COALESCE(MAX(rowid),0) FROM explorer_objects",[],|r|r.get(0)).map_err(failure)?,sequence:i64::MAX,id:"~".into()}}else{decode_cursor(&q.cursor)?};
    if cursor.filter!=filter{return Err("CONFLICT: cursor filter changed; refresh".into());}
    let mut s=c.prepare("SELECT sequence,id,document FROM explorer_objects WHERE object_type=? AND rowid<=? AND (sequence<? OR (sequence=? AND id<?)) AND (?='' OR goal_id=?) AND (?='' OR model_root=?) AND (?='' OR actor=?) AND (?='' OR status=?) AND (?='' OR namespace=?) AND (?='' OR json_extract(document,'$.campaignId')=?) AND at>=? AND (?=0 OR at<=?) ORDER BY sequence DESC,id DESC LIMIT ?").map_err(failure)?;
    let rows=s.query_map(params![kind,cursor.snapshot,cursor.sequence,cursor.sequence,cursor.id,q.goal_id,q.goal_id,q.model_root,q.model_root,q.miner_id,q.miner_id,q.status,q.status,q.namespace,q.namespace,q.campaign_id,q.campaign_id,q.from,q.to,q.to,limit+1],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(failure)?.collect::<rusqlite::Result<Vec<_>>>().map_err(failure)?;
    let more=rows.len()>limit;let page=&rows[..rows.len().min(limit)];let items=page.iter().map(|x|json_value(x.2.clone())).collect::<Result<Vec<_>>>()?;
    let next=if more {page.last().map(|(seq,id,_)|encode_cursor(&Cursor{version:INDEX_VERSION.into(),filter,snapshot:cursor.snapshot,sequence:*seq,id:id.clone()}))}else{None};
    let mut v=wrapped(c,json!(items))?;v["items"]=json!(items);v["hasMore"]=json!(more);v["nextCursor"]=json!(next);v["snapshot"]["objectWatermark"]=json!(cursor.snapshot.to_string());Ok(v)
}
fn object(c:&Connection,kind:&str,id:&str,namespace:Option<&str>)->Result<Value>{
    let s:Option<String>=c.query_row("SELECT document FROM explorer_objects WHERE object_type=? AND id=? AND (? IS NULL OR namespace=?)",params![kind,id,namespace,namespace],|r|r.get(0)).optional().map_err(failure)?;s.map(json_value).transpose()?.ok_or("NOT_FOUND: object within project index".into())
}
fn detail(c:&Connection,path:&str,q:&ExplorerQuery)->Result<Value>{
    let parts:Vec<_>=path.trim_matches('/').split('/').collect();
    if parts.len()==2&&["records","rounds","candidates"].contains(&parts[0]) {p::root(parts[1]).map_err(|_|"INVALID_INPUT: object ID")?;let kind=match parts[0]{"records"=>"record","rounds"=>"round",_=>"candidate"};return wrapped(c,object(c,kind,parts[1],Some("target-v2"))?);}
    if parts.len()==3&&parts[0]=="records"&&parts[2]=="evidence"{return wrapped(c,object(c,"record",parts[1],Some("target-v2"))?["evidence"].clone());}
    if parts.len()>=3&&parts[0]=="chains" {
        let ns=parts[1];if ns.len()>100||!ns.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_:".contains(&b)){return Err("INVALID_INPUT: chain namespace".into());}
        let state:Option<String>=c.query_row("SELECT state FROM explorer_chain_sources WHERE namespace=?",[ns],|r|r.get(0)).optional().map_err(failure)?;
        if state.is_none(){return Err("NOT_CONFIGURED: chain namespace is not bound to an audited genesis/program".into());}
        if parts.len()==3&&parts[2]=="transactions"{let mut filter=q.clone();filter.namespace=ns.into();return list(c,"transaction",&filter);}
        if parts.len()==4 {
            let kind=match parts[2]{"tx"=>"transaction","blocks"=>"block","addresses"=>"address","stages"=>"stage","rights"=>"right","receipts"=>"receipt",_=>return Err("NOT_FOUND: chain route".into())};
            let width=base58_width(parts[3]);if kind=="block" {parts[3].parse::<u64>().map_err(|_|"INVALID_INPUT: native slot")?;}else if kind=="transaction" {if width!=Some(64){return Err("INVALID_INPUT: transaction signature width".into());}}else if !["stage","receipt"].contains(&kind)&&width!=Some(32){return Err("INVALID_INPUT: public address width".into());}
            let mut data=object(c,kind,parts[3],Some(ns))?;let observed=data["sourceObservedAt"].as_u64().unwrap_or(0);let freshness=if data["sourceError"].is_string(){"unavailable"}else if now().saturating_sub(observed)>60{"stale"}else{"fresh"};data["dataFreshness"]=json!(freshness);data["states"]["data"]=json!(freshness);let mut response=wrapped(c,data)?;response["freshness"]=json!(freshness);response["source"]=json!({"chainNamespace":ns,"actualSourceObservedAt":observed,"scope":"project RPC index; historical account access not implied"});return Ok(response);
        }
    }
    Err("NOT_FOUND: Explorer route".into())
}
fn search(c:&Connection,q:&ExplorerQuery)->Result<Value>{
    let input=q.q.trim();if input.is_empty(){return wrapped(c,json!({"matches":[],"hints":["record:","round:","result:","tx:","address:","slot:","body:"]}));}
    let mut prefix="";let mut term=input;if let Some((a,b))=input.split_once(':'){if ["record","round","result","goal","contribution","miner","tx","address","slot","body"].contains(&a){prefix=a;term=b;}}
    let alias=if prefix=="body"{format!("body:{term}")}else{term.into()};
    let mut s=c.prepare("SELECT DISTINCT a.namespace,a.object_type,a.id,o.document FROM explorer_aliases a JOIN explorer_objects o ON a.namespace=o.namespace AND a.object_type=o.object_type AND a.id=o.id WHERE a.alias=? AND (?='' OR a.namespace=?) LIMIT 50").map_err(failure)?;
    let rows=s.query_map(params![alias,q.namespace,q.namespace],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).map_err(failure)?.collect::<rusqlite::Result<Vec<_>>>().map_err(failure)?;
    let mut matches=Vec::new();for (ns,kind,id,s) in rows{if prefix.is_empty()||prefix==kind||["goal","contribution","body"].contains(&prefix){let v=json_value(s)?;matches.push(json!({"namespace":ns,"objectType":kind,"id":id,"status":v["status"],"goalId":v["goalId"]}));}}
    if prefix=="miner" {p::root(term).map_err(|_|"INVALID_INPUT: protocol miner hex")?;let records=list(c,"record",&ExplorerQuery{miner_id:term.into(),limit:20,..Default::default()})?;for record in records["items"].as_array().unwrap(){matches.push(json!({"namespace":"target-v2","objectType":"record","id":record["recordId"],"status":record["status"]}));}}
    if matches.is_empty()&&term.chars().count()>=3&&!term.bytes().all(|b|b.is_ascii_hexdigit()) {
        let literal=format!("\"{}\"",term.replace('"',"\"\""));let mut statement=c.prepare("SELECT namespace,object_type,id FROM explorer_text WHERE explorer_text MATCH ? LIMIT 50").map_err(failure)?;
        for row in statement.query_map([literal],|r|Ok(json!({"namespace":r.get::<_,String>(0)?,"objectType":r.get::<_,String>(1)?,"id":r.get::<_,String>(2)?}))).map_err(failure)?{matches.push(row.map_err(failure)?);}
    }
    let mut hints=Vec::new();let width=base58_width(term);
    if term.bytes().all(|b|b.is_ascii_digit())&&term.parse::<u64>().is_ok(){hints.push(json!({"type":"body","value":term,"url":format!("/atlas/?body={term}")}));hints.push(json!({"type":"native-slot","value":term,"requires":"explicit chain namespace; real RPC block"}));let mut stmt=c.prepare("SELECT namespace,object_type,id FROM explorer_objects WHERE sequence=? AND object_type IN ('record','round') LIMIT 20").map_err(failure)?;for row in stmt.query_map([term.parse::<i64>().unwrap_or(-1)],|r|Ok(json!({"namespace":r.get::<_,String>(0)?,"objectType":r.get::<_,String>(1)?,"id":r.get::<_,String>(2)?}))).map_err(failure)?{if prefix.is_empty(){matches.push(row.map_err(failure)?);}}}
    if width==Some(32)||width==Some(64){hints.push(json!({"type":if width==Some(32){"solana-address"}else{"solana-transaction"},"decodedBytes":width,"value":term,"requires":"choose a configured chain namespace; no miner-hex-to-wallet mapping is inferred"}));}
    if term.len()==64&&p::root(term).is_ok(){hints.push(json!({"type":"protocol-hex","value":term,"ambiguous":true}));}
    wrapped(c,json!({"matches":matches,"hints":hints,"scope":"indexed project objects and authorized publications","noMatch":matches.is_empty(),"chainNamespace":if q.namespace.is_empty(){None}else{Some(&q.namespace)}}))
}
impl Authority{
    pub fn project_explorer(&self)->Result<Value>{std::fs::create_dir_all(self.data.join("public-query")).map_err(|e|e.to_string())?;explorer_projection::project(&self.data,&self.data.join("public-query/explorer-v1.sqlite"),&self.config.network_id,&self.authority_id,&self.config.campaign_id)}
    pub fn explorer_query(&self,path:&str,q:&ExplorerQuery)->Result<Value>{let c=explorer_projection::source_open(&self.data.join("public-query/explorer-v1.sqlite"))?;c.execute_batch("BEGIN").map_err(failure)?;let v=query_index(&c,path,q);c.execute_batch("COMMIT").map_err(failure)?;v}
}
struct Error(String);impl IntoResponse for Error{fn into_response(self)->Response{let code=self.0.split(':').next().unwrap_or("INDEX_UNAVAILABLE");let status=match code{"NOT_FOUND"=>StatusCode::NOT_FOUND,"INVALID_INPUT"=>StatusCode::BAD_REQUEST,"CONFLICT"=>StatusCode::CONFLICT,_=>StatusCode::SERVICE_UNAVAILABLE};(status,Json(json!({"code":code,"message":code,"retryable":status==StatusCode::SERVICE_UNAVAILABLE,"data":null,"freshness":"unavailable"}))).into_response()}}
async fn query(State(a):State<Arc<Authority>>,Path(path):Path<String>,Query(q):Query<ExplorerQuery>)->std::result::Result<Json<Value>,Error>{tokio::task::spawn_blocking(move||a.explorer_query(&path,&q)).await.map_err(|_|Error("INDEX_UNAVAILABLE".into()))?.map(Json).map_err(Error)}
async fn stream(State(a):State<Arc<Authority>>,Query(q):Query<ExplorerQuery>,headers:HeaderMap)->Sse<impl futures_util::Stream<Item=std::result::Result<Event,std::convert::Infallible>>>{
    let after=headers.get("last-event-id").and_then(|h|h.to_str().ok()).and_then(|s|s.parse().ok()).unwrap_or(q.after);let rx=a.event_bus.subscribe();
    // Shared authority outbox broadcast is only a wake-up signal. Replay uses
    // the independent projection and bounded cursor batches, not per-client DB polling.
    let state=(a,after,rx,std::collections::VecDeque::<Value>::new(),true);
    let stream=futures_util::stream::unfold(state,|(a,mut after,mut rx,mut queue,mut read)|async move{loop{
        if let Some(item)=queue.pop_front(){after=item["cursor"].as_str().unwrap().parse().unwrap_or(after);let event=Event::default().event("explorer-event").id(after.to_string()).json_data(item).unwrap();return Some((Ok(event),(a,after,rx,queue,read)));}
        if !read{if rx.recv().await.is_err(){return None;}tokio::time::sleep(std::time::Duration::from_secs(2)).await;}
        let copy=a.clone();if let Ok(Ok(v))=tokio::task::spawn_blocking(move||copy.explorer_query("events",&ExplorerQuery{after,..Default::default()})).await{queue.extend(v["items"].as_array().unwrap().iter().cloned());read=v["hasMore"]==true;}else{read=false;}
        if queue.is_empty(){read=false;}
    }});Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
pub fn routes()->Router<Arc<Authority>>{Router::new().route("/api/explorer/v1/stream",get(stream)).route("/api/explorer/v1/{*path}",get(query))}

// Dedicated production visitor process. The original merged routes remain
// compatible for development, but public nginx sends Explorer to this reader.
#[derive(Clone)]pub struct QueryService{pub index:std::path::PathBuf,pub notifications:tokio::sync::broadcast::Sender<u64>}
async fn standalone_query(State(service):State<QueryService>,Path(path):Path<String>,Query(q):Query<ExplorerQuery>)->std::result::Result<Json<Value>,Error>{
 tokio::task::spawn_blocking(move||{let c=explorer_projection::source_open(&service.index)?;c.execute_batch("BEGIN").map_err(failure)?;let result=query_index(&c,&path,&q);c.execute_batch("COMMIT").map_err(failure)?;result}).await.map_err(|_|Error("INDEX_UNAVAILABLE".into()))?.map(Json).map_err(Error)
}
async fn standalone_stream(State(service):State<QueryService>,Query(q):Query<ExplorerQuery>,headers:HeaderMap)->Sse<impl futures_util::Stream<Item=std::result::Result<Event,std::convert::Infallible>>>{
 let after=headers.get("last-event-id").and_then(|h|h.to_str().ok()).and_then(|s|s.parse().ok()).unwrap_or(q.after);let rx=service.notifications.subscribe();
 let stream=futures_util::stream::unfold((service,after,rx,std::collections::VecDeque::<Value>::new(),true),|(service,mut after,mut rx,mut queue,mut read)|async move{loop{
  if let Some(item)=queue.pop_front(){after=item["cursor"].as_str().unwrap().parse().unwrap_or(after);let event=Event::default().event("explorer-event").id(after.to_string()).json_data(item).unwrap();return Some((Ok(event),(service,after,rx,queue,read)));}
  if !read{match rx.recv().await{Ok(_)|Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>{},Err(_)=>return None}}
  let path=service.index.clone();if let Ok(Ok(v))=tokio::task::spawn_blocking(move||{let c=explorer_projection::source_open(&path)?;query_index(&c,"events",&ExplorerQuery{after,..Default::default()})}).await{queue.extend(v["items"].as_array().unwrap().iter().cloned());read=v["hasMore"]==true;}else{read=false;}
  if queue.is_empty(){read=false;}
 }});Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
pub fn standalone_routes(index:std::path::PathBuf)->Router{
 let service=QueryService{index,notifications:tokio::sync::broadcast::channel(64).0};let monitor=service.clone();
 tokio::spawn(async move{let mut interval=tokio::time::interval(std::time::Duration::from_secs(2));let mut cursor=0u64;loop{interval.tick().await;let path=monitor.index.clone();if let Ok(Ok(last))=tokio::task::spawn_blocking(move||{let c=explorer_projection::source_open(&path)?;c.query_row("SELECT COALESCE(MAX(sequence),0) FROM explorer_events",[],|r|r.get::<_,u64>(0)).map_err(failure)}).await{if last>cursor{cursor=last;let _=monitor.notifications.send(cursor);}}}});
 Router::new().route("/api/explorer/v1/stream",get(standalone_stream)).route("/api/explorer/v1/{*path}",get(standalone_query))
 .layer(axum::middleware::from_fn(|request:axum::extract::Request,next:axum::middleware::Next|async move{let mut response=next.run(request).await;response.headers_mut().insert("access-control-allow-origin",axum::http::HeaderValue::from_static("*"));response.headers_mut().insert("cache-control",axum::http::HeaderValue::from_static("no-store"));response.headers_mut().insert("x-content-type-options",axum::http::HeaderValue::from_static("nosniff"));response})).with_state(service)
}
