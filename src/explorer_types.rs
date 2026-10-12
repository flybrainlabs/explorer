//! Exact identities, public states and query cursors shared by the projector.
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use flypow_target_protocol::{id,Result};
pub const INDEX_VERSION:&str="flybrain-explorer-v1.0";
pub fn record_id(network:&str,authority:&str,campaign:&str,round:&str,claim:&str)->Result<String>{
    id("compute-record-v1",&json!({"namespace":"target-v2","networkId":network,"authorityId":authority,"campaignId":campaign,"roundId":round,"winnerClaimId":claim}))
}
pub fn states(competition:&str,published:bool,right:Option<&Value>)->Value{
    let entitlement=right.map(|r|if r["released"]==true{"released"}else if r["accepted"]!=true{"pending-acceptance"}else if r["paid"]==r["amount"]{"paid"}else if r["paid"]!="0"{"partially-paid"}else{"confirmed"}).unwrap_or("not-linked");
    json!({"computation":if ["valid-winner","valid-loser"].contains(&competition){"completed"}else if competition=="invalid"{"failed"}else if competition=="timed-out"{"terminated"}else{"in-progress"},"competition":competition,"publication":if published{"authorized-public"}else{"private"},"entitlement":entitlement,"chain":"not-linked","data":"fresh"})
}
#[derive(Default,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ExplorerQuery{
    #[serde(default)]pub cursor:String,#[serde(default)]pub limit:usize,
    #[serde(default)]pub goal_id:String,#[serde(default)]pub model_root:String,
    #[serde(default)]pub miner_id:String,#[serde(default)]pub status:String,
    #[serde(default)]pub campaign_id:String,#[serde(default)]pub namespace:String,
    #[serde(default)]pub from:u64,#[serde(default)]pub to:u64,#[serde(default)]pub q:String,
    #[serde(default)]pub after:u64,
}
#[derive(Serialize,Deserialize)]pub struct Cursor{pub version:String,pub filter:String,pub snapshot:i64,pub sequence:i64,pub id:String}
pub fn filter_key(kind:&str,q:&ExplorerQuery)->Result<String>{id("explorer-query-v1",&json!([kind,q.goal_id,q.model_root,q.miner_id,q.status,q.campaign_id,q.namespace,q.from,q.to]))}
pub fn encode_cursor(c:&Cursor)->String{flypow_target_protocol::hex(serde_json::to_string(c).unwrap().as_bytes())}
pub fn decode_cursor(text:&str)->Result<Cursor>{if text.len()>4096{return Err("INVALID_INPUT: cursor length".into());}let c:Cursor=serde_json::from_slice(&flypow_target_protocol::unhex(text)?).map_err(|_|"INVALID_INPUT: cursor")?;if c.version!=INDEX_VERSION{return Err("CONFLICT: cursor index version; refresh".into());}Ok(c)}
pub fn base58_width(text:&str)->Option<usize>{
    if text.is_empty()||text.len()>90{return None;}
    let alphabet=b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";let mut bytes=vec![0u8];
    for ch in text.bytes(){let mut carry=alphabet.iter().position(|b|*b==ch)? as u32;for byte in bytes.iter_mut().rev(){carry+=(*byte as u32)*58;*byte=(carry&255) as u8;carry>>=8;}while carry>0{bytes.insert(0,(carry&255) as u8);carry>>=8;}}
    let leading=text.bytes().take_while(|b|*b==b'1').count();let meaningful=bytes.iter().position(|b|*b!=0).map(|n|bytes.len()-n).unwrap_or(0);Some(leading+meaningful)
}
