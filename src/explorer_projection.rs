//! A rebuildable, separate SQLite/WAL public projection. The source is read-only.
//! Public objects are constructed field-by-field. No private payload is exported.
use crate::explorer_types::*;
use flypow_target_protocol::{self as p,Result};
use rusqlite::{Connection,OpenFlags,OptionalExtension,params};
use serde_json::{Value,json};
use ed25519_dalek::{Signature,VerifyingKey};
use std::path::Path;
fn err(e:rusqlite::Error)->String{format!("INDEX_UNAVAILABLE: {e}")}
fn val(s:String)->Result<Value>{serde_json::from_str(&s).map_err(|_|"INDEX_UNAVAILABLE: source JSON".into())}
fn check_signature(v:&Value,domain:&str,authority:&str)->Result<()> {
    let mut message=v.clone();let signature=message.as_object_mut().ok_or("source signature object")?.remove("signature").ok_or("source signature missing")?;
    if message["authorityId"].is_string()&&message["authorityId"]!=authority{return Err("source authority mismatch".into());}
    let key=VerifyingKey::from_bytes(&p::unhex(authority)?.try_into().map_err(|_|"source key")?).map_err(|_|"source key")?;
    key.verify_strict(&p::unhex(&p::id(domain,&message)?)?,&Signature::from_slice(&p::unhex(signature.as_str().ok_or("source signature")?)?).map_err(|_|"source signature")?).map_err(|_|"INDEX_UNAVAILABLE: source signature rejected".into())
}
pub fn source_open(path:&Path)->Result<Connection>{let c=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY|OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(err)?;c.busy_timeout(std::time::Duration::from_secs(2)).map_err(err)?;Ok(c)}
fn all(c:&Connection,sql:&str)->Result<Vec<Value>>{let mut q=c.prepare(sql).map_err(err)?;let rows=q.query_map([],|r|r.get::<_,String>(0)).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;rows.into_iter().map(val).collect()}
fn checkpoint(c:&Connection,source:&str)->Result<i64>{Ok(c.query_row("SELECT sequence FROM explorer_checkpoints WHERE source=?",[source],|r|r.get(0)).optional().map_err(err)?.unwrap_or(0))}
pub fn put(c:&Connection,namespace:&str,kind:&str,id:&str,sequence:i64,at:i64,v:&Value)->Result<()> {
    let goal=v["goalId"].as_str().unwrap_or("");let model=v["modelRoot"].as_str().unwrap_or("");let actor=v["minerId"].as_str().unwrap_or("");let status=v["status"].as_str().unwrap_or("");
    c.execute("INSERT INTO explorer_objects VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(namespace,object_type,id) DO UPDATE SET status=excluded.status,document=excluded.document",params![namespace,kind,id,sequence,at,goal,model,actor,status,v.to_string()]).map_err(err)?;
    let search_text=format!("{} {} {} {}",v["description"].as_str().unwrap_or(""),v["goal"]["description"].as_str().unwrap_or(""),v["researchGoal"].as_object().map(|o|o.values().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default(),v["title"].as_object().map(|o|o.values().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default());
    let text_row:Option<i64>=c.query_row("SELECT text_rowid FROM explorer_text_links WHERE namespace=? AND object_type=? AND id=?",params![namespace,kind,id],|r|r.get(0)).optional().map_err(err)?;
    if let Some(row)=text_row {c.execute("DELETE FROM explorer_text WHERE rowid=?",[row]).map_err(err)?;c.execute("INSERT INTO explorer_text(rowid,namespace,object_type,id,content) VALUES(?,?,?,?,?)",params![row,namespace,kind,id,search_text]).map_err(err)?;}else{c.execute("INSERT INTO explorer_text(namespace,object_type,id,content) VALUES(?,?,?,?)",params![namespace,kind,id,search_text]).map_err(err)?;let row=c.last_insert_rowid();c.execute("INSERT INTO explorer_text_links VALUES(?,?,?,?)",params![namespace,kind,id,row]).map_err(err)?;}
    c.execute("INSERT OR IGNORE INTO explorer_aliases VALUES(?,?,?,?)",params![id,namespace,kind,id]).map_err(err)?;
    for key in ["goalId","scientificTaskId","claimId","candidateId","readoutId","contributionId","acceptanceId","publicResultId","slotPda","stagePda"] {if let Some(alias)=v[key].as_str(){c.execute("INSERT OR IGNORE INTO explorer_aliases VALUES(?,?,?,?)",params![alias,namespace,kind,id]).map_err(err)?;}}
    Ok(())
}
fn scalar(c:&Connection,q:&str)->Result<i64>{c.query_row(q,[],|r|r.get(0)).map_err(err)}
pub fn project(data:&Path,index:&Path,network:&str,authority:&str,campaign:&str)->Result<Value>{
    let source=source_open(&data.join("target-v2.sqlite"))?;
    source.execute_batch("BEGIN").map_err(err)?;
    let mut db=Connection::open(index).map_err(err)?;db.busy_timeout(std::time::Duration::from_secs(2)).map_err(err)?;db.execute_batch(include_str!("../config/explorer-v1.sql")).map_err(err)?;
    let protocol=scalar(&source,"SELECT COALESCE(MAX(event_sequence),0) FROM protocol_events")?;let product=scalar(&source,"SELECT COALESCE(MAX(sequence),0) FROM product_audit")?;let economy=scalar(&source,"SELECT COALESCE(MAX(sequence),0) FROM economic_events")?;
    let previous=checkpoint(&db,"authority-protocol")?;
    let at=crate::target_authority::now() as i64;
    let tx=db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    // Frozen rounds and changed historical candidate state are indexed in source
    // transactions. A one-time bootstrap backfills existing history with provenance.
    let boot=tx.query_row("SELECT value FROM explorer_meta WHERE key='bootstrap'",[],|r|r.get::<_,String>(0)).optional().map_err(err)?.is_none();
    let query=if boot{"SELECT descriptor_json FROM target_rounds ORDER BY sequence".into()}else{format!("SELECT descriptor_json FROM target_rounds WHERE round_id IN (SELECT round_id FROM protocol_events WHERE event_sequence>{previous}) OR status IN ('OPEN','VERIFYING') ORDER BY sequence")};
    let rounds=all(&source,&query)?;
    for r in rounds {
        p::validate_round(&r)?;check_signature(&r,"round-descriptor",authority)?;
        if r["pre"]["networkId"]!=network||r["pre"]["authorityId"]!=authority{return Err("source round namespace mismatch".into());}
        let rid=r["roundId"].as_str().ok_or("source round identity")?;
        let (status,seq,opened):(String,i64,i64)=source.query_row("SELECT status,sequence,opened_at FROM target_rounds WHERE round_id=?",[rid],|x|Ok((x.get(0)?,x.get(1)?,x.get(2)?))).map_err(err)?;
        let winner:Option<Value>=source.query_row("SELECT winner_json FROM round_winners WHERE round_id=?",[rid],|x|x.get::<_,String>(0)).optional().map_err(err)?.map(val).transpose()?;
        let mut candidates=Vec::new();let mut q=source.prepare("SELECT c.claim_id,c.miner_id,c.status,c.claim_sequence,c.received_at,c.reveal_deadline,c.verification_deadline,t.reason FROM candidate_claims c LEFT JOIN candidate_terminal t USING(claim_id) WHERE c.round_id=? ORDER BY c.claim_sequence").map_err(err)?;
        let rows=q.query_map([rid],|x|Ok((x.get::<_,String>(0)?,x.get::<_,String>(1)?,x.get::<_,String>(2)?,x.get::<_,i64>(3)?,x.get::<_,i64>(4)?,x.get::<_,i64>(5)?,x.get::<_,i64>(6)?,x.get::<_,Option<String>>(7)?))).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;drop(q);
        for (claim,miner,state,order,received,reveal,verify,reason) in rows {
            let is_winner=winner.as_ref().is_some_and(|w|w["claimId"]==claim);
            let competition=if state=="VALID"{if is_winner{"valid-winner"}else if winner.is_some(){"valid-loser"}else{"verified-awaiting-earlier-commitment"}}else{match state.as_str(){"COMMITTED"=>"committed","REVEALED"=>"verifying","TIMED_OUT"=>"timed-out",_=>"invalid"}};
            let votes=all(&source,&format!("SELECT vote_json FROM verification_votes WHERE claim_id='{claim}' ORDER BY received_at,verifier_id"))?;
            let mut reports=Vec::new();for vote in votes {let signed:crate::target_authority::SignedRequest=serde_json::from_value(vote.clone()).map_err(|_|"source vote")?;signed.authenticate("verifier-vote")?;
                reports.push(json!({"verifierId":signed.public_key,"valid":signed.payload["valid"],"fullReplay":signed.payload["fullReplay"],"completeTrials":signed.payload["completeTrials"],"goalResultRoot":signed.payload["goalResultRoot"],"sourceSignedMessageHash":p::id("authority-action",&signed.unsigned())?,"scope":"authorized-full-model-replay; original lease envelope private"}));
            }
            let declarations=all(&source,&format!("SELECT declaration FROM public_verification_declarations WHERE claim_id='{claim}' ORDER BY verifier_id"))?;
            for declaration in &declarations{check_signature(declaration,"public-verification-declaration-v1",authority)?;}
            let c=json!({"schemaVersion":1,"protocolVersion":2,"namespace":"target-v2","objectType":"candidate","nativeId":claim,"claimId":claim,"roundId":rid,"goalId":r["pre"]["goalId"],"minerId":miner,"claimSequence":order.to_string(),"receivedAt":received,"revealDeadline":reveal,"verificationDeadline":verify,"status":competition,"states":states(competition,false,None),"outcomeReason":reason.unwrap_or_else(||if competition=="valid-loser"{"earlier-eligible-verified-commitment-won".into()}else if competition=="invalid"{"full-replay-rejected".into()}else{competition.into()}),"verificationReports":reports,"publicVerificationDeclarations":declarations,"publicationPolicy":"protocol-metadata-only","evidenceVisibility":"private-input-and-artifact","sourceLocator":{"claimId":claim},"projectedAt":at});
            put(&tx,"target-v2","candidate",&claim,order,received,&c)?;candidates.push(c);
        }
        let events=all(&source,&format!("SELECT payload_json FROM protocol_events WHERE round_id='{rid}' ORDER BY event_sequence"))?.into_iter().map(|e|json!({"eventSequence":e["eventSequence"].as_i64().map(|n|n.to_string()),"kind":e["kind"],"createdAt":e["createdAt"],"roundId":e["roundId"]})).collect::<Vec<_>>();
        let round=json!({"schemaVersion":1,"namespace":"target-v2","objectType":"round","nativeId":rid,"roundId":rid,"campaignId":r["pre"]["campaignId"],"goalId":r["pre"]["goalId"],"modelRoot":r["pre"]["modelRoot"],"displaySequence":seq.to_string(),"status":status,"descriptor":r,"winner":winner,"candidates":candidates,"protocolEvents":events,"sourceObservedAt":at,"projectedAt":at,"backfilledAt":if boot{Some(at)}else{None},"publicationPolicy":"protocol-metadata-only"});
        put(&tx,"target-v2","round",rid,seq,opened,&round)?;
        if let Some(w)=winner {
            check_signature(&w,"round-winner",authority)?;
            let claim=w["claimId"].as_str().ok_or("winner claim")?;let record=record_id(network,authority,campaign,rid,claim)?;
            let science:Option<(String,String,String)>=source.query_row("SELECT c.id,c.readout_id,a.id FROM science_contributions c JOIN science_acceptances a ON a.contribution_id=c.id WHERE c.authorization=?",[claim],|x|Ok((x.get(0)?,x.get(1)?,x.get(2)?))).optional().map_err(err)?;
            let (contribution,readout,acceptance)=science.unwrap_or_default();
            let readout_descriptor:Option<Value>=source.query_row("SELECT descriptor FROM science_readouts WHERE id=?",[&readout],|r|r.get::<_,String>(0)).optional().map_err(err)?.map(val).transpose()?;
            let public:Option<String>=source.query_row("SELECT id FROM public_results WHERE id=?",[&readout],|x|x.get(0)).optional().map_err(err)?;
            let declarations=all(&source,&format!("SELECT declaration FROM public_verification_declarations WHERE claim_id='{claim}' ORDER BY verifier_id"))?;
            let evidence=json!({"verificationDeclarations":declarations,"proofs":[{"domain":"round-descriptor","publicKey":authority,"signedMessage":r},{"domain":"round-winner","publicKey":authority,"signedMessage":w}],"verificationScope":"source signatures and immutable hashes; full scientific replay is a separate explicit action","goalResultRoot":w["goalResultRoot"],"privateMaterials":"not-published"});
            let v=json!({"schemaVersion":1,"protocolVersion":2,"namespace":"target-v2","networkId":network,"authorityId":authority,"objectType":"record","recordId":record,"nativeId":claim,"displaySequence":seq.to_string(),"campaignId":campaign,"roundId":rid,"claimId":claim,"candidateId":w["candidateId"],"goalId":w["goalId"],"scientificTaskId":crate::product_identity::task(&serde_json::from_value(r["goal"].clone()).map_err(|_|"source goal")?)?,"minerId":w["minerId"],"simulationIds":readout_descriptor.as_ref().map(|v|v["simulationIds"].clone()).unwrap_or(json!([])),"readoutId":readout,"contributionId":contribution,"acceptanceId":acceptance,"publicResultId":public,"modelRoot":r["pre"]["modelRoot"],"graphRoot":r["pre"]["graphRoot"],"parameterProfileHash":r["pre"]["parameterProfileHash"],"evaluatorVersion":r["pre"]["evaluatorVersion"],"verificationPolicy":r["pre"]["verificationPolicy"],"winner":true,"status":"valid-winner","states":states("valid-winner",public.is_some(),None),"researchGoal":{"zh":"果蝇神经网络刺激响应研究","en":"Fruit-fly neural network stimulus-response research"},"goal":r["goal"],"completedAt":w["confirmedAt"],"evidence":evidence,"sourceLocator":{"authorityId":authority,"roundId":rid,"claimId":claim},"sourceObservedAt":at,"projectedAt":at,"indexVersion":INDEX_VERSION,"backfilledAt":if boot{Some(at)}else{None},"publicationPolicy":"protocol-metadata-only","evidenceVisibility":if public.is_some(){"authorized-public-package"}else{"private-scientific-output"},"economicLinks":[],"chainLinks":[]});
            put(&tx,"target-v2","record",&record,seq,w["confirmedAt"].as_i64().unwrap_or(at),&v)?;
        }
    }
    if boot||protocol!=previous {let mut q=source.prepare("SELECT goal_id,spec_json FROM goal_specs ORDER BY goal_id").map_err(err)?;for row in q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(err)?{let(gid,encoded)=row.map_err(err)?;let goal:p::GoalSpec=serde_json::from_str(&encoded).map_err(|_|"goal")?;let task=crate::product_identity::task(&goal)?;let state:Option<String>=source.query_row("SELECT state FROM goal_plan_progress WHERE campaign_id=? AND goal_id=?",params![campaign,gid],|r|r.get(0)).optional().map_err(err)?;put(&tx,"science-v1","goal",&gid,0,at,&json!({"goalId":gid,"scientificTaskId":task,"modelRoot":goal.model_root,"description":goal.description,"status":state.unwrap_or("REGISTERED".into()),"legalInputCount":goal.inputs().len().to_string(),"researchGoal":{"en":"Fruit-fly neural network stimulus-response research","zh":"果蝇神经网络刺激响应研究"},"publicationPolicy":"published-frozen-scientific-task-definition"}))?;}}
    if boot||checkpoint(&tx,"product-audit")?!=product {for r in all(&source,"SELECT summary FROM public_results ORDER BY sequence")? {
        let rid=r["id"].as_str().ok_or("public result")?;let seq:i64=source.query_row("SELECT sequence FROM public_results WHERE id=?",[rid],|x|x.get(0)).map_err(err)?;put(&tx,"science-v1","result",rid,seq,r["publishedAt"].as_i64().unwrap_or(at),&r)?;
        // Publications can be added independently after the original winner.
        tx.execute("UPDATE explorer_objects SET document=json_set(document,'$.publicResultId',?,'$.states.publication','authorized-public','$.evidenceVisibility','authorized-public-package') WHERE object_type='record' AND json_extract(document,'$.readoutId')=?",params![rid,rid]).map_err(err)?;
    }}
    // Public envelope manifests are independent signed declarations, never a
    // redacted form of a private signature payload.
    for manifest in all(&source,"SELECT json_object('root',root,'manifest',json(manifest),'signature',signature,'at',created_at) FROM evidence_manifests ORDER BY root")? {
        let digest=crate::evidence_manifest::digest(&manifest["manifest"])?;if digest!=manifest["root"]{return Err("INDEX_UNAVAILABLE: evidence envelope digest".into());}
        put(&tx,"science-v1","evidence",&digest,0,manifest["at"].as_i64().unwrap_or(at),&json!({"root":digest,"manifest":manifest["manifest"],"signature":manifest["signature"],"authorityId":authority,"domain":"public-evidence-manifest-v1","scope":"public outer envelope; private commitments not disclosed"}))?;
    }
    let chains=all(&tx,"SELECT json_object('namespace',namespace,'genesisHash',genesis,'state',state,'updatedAt',updated_at,'configuration',json(configuration)) FROM explorer_chain_sources ORDER BY namespace")?;
    if boot||checkpoint(&tx,"economic-events")?!=economy {
        let books=all(&source,"SELECT json_object('asset',asset,'book',json(state),'root',root) FROM economic_books ORDER BY asset")?;
        for value in books {
            let book:flypow_economic_core::Book=serde_json::from_value(value["book"].clone()).map_err(|_|"source economic book")?;book.reconcile().map_err(|_|"source liability mismatch")?;
            for (key,slot) in &book.slots {
                let Some(chain)=chains.iter().find(|c|c["genesisHash"]==slot.genesis_hash&&c["configuration"]["programId"]==slot.program_id)else{continue;};let ns=chain["namespace"].as_str().unwrap();
                let right=json!({"slotPda":slot.slot_pda,"economicSlotKey":{"version":1,"genesisHash":slot.genesis_hash,"programId":slot.program_id,"economicTaskId":slot.economic_task,"kind":slot.kind,"unitId":slot.unit_id},"assetId":book.asset,"stageId":slot.stage,"amountAtoms":slot.amount.0.to_string(),"paidAtoms":slot.paid.0.to_string(),"inflightAtoms":slot.inflight.0.to_string(),"claimableAtoms":if slot.accepted{Some((slot.amount.0-slot.paid.0-slot.inflight.0).to_string())}else{None},"beneficiary":slot.beneficiary,"contributionId":slot.contribution,"accepted":slot.accepted,"released":slot.released,"status":if slot.released{"released"}else if !slot.accepted{"pending-acceptance"}else if slot.paid.0==slot.amount.0{"paid"}else{"confirmed"},"localLedgerReconciled":true,"chainStateReconciled":false,"scientificVerification":"evidence-manifest-required","sourceRoot":value["root"],"payments":book.payments.iter().filter(|(_,p)|p.slot==*key).map(|(k,p)|json!({"consumptionKey":k,"paymentId":p.payment_id,"signature":p.transaction,"amountAtoms":p.amount.0.to_string(),"status":p.status})).collect::<Vec<_>>()});
                // Never infer a private protocol-miner↔wallet mapping. Only an
                // admitted public chain fact's contribution binding is used.
                let mut stmt=tx.prepare("SELECT id,document FROM explorer_objects WHERE object_type='record' AND json_extract(document,'$.contributionId')=?").map_err(err)?;
                let matches=stmt.query_map([&slot.contribution],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;drop(stmt);
                for (record_id,document) in matches {let mut record=val(document)?;let mut links=record["economicLinks"].as_array().cloned().unwrap_or_default();links.retain(|l|l["slotPda"]!=slot.slot_pda);links.push(json!({"namespace":ns,"slotPda":slot.slot_pda,"stageId":slot.stage,"entitlement":right}));record["economicLinks"]=json!(links);record["states"]["entitlement"]=right["status"].clone();tx.execute("UPDATE explorer_objects SET document=? WHERE namespace='target-v2' AND object_type='record' AND id=?",params![record.to_string(),record_id]).map_err(err)?;}
            }
        }
    }
    if boot||checkpoint(&tx,"product-audit")?!=product {
    let neurons=all(&source,"SELECT json_object('resultId',result_id,'bodyId',body_id,'role',role) FROM result_neurons ORDER BY result_id,body_id,role")?;
    for relation in neurons {tx.execute("INSERT OR IGNORE INTO explorer_aliases VALUES(?,'science-v1','result',?)",params![format!("body:{}",relation["bodyId"].as_str().unwrap()),relation["resultId"].as_str().unwrap()]).map_err(err)?;}
    }
    let mut q=source.prepare("SELECT event_sequence,payload_json FROM protocol_events WHERE event_sequence>? ORDER BY event_sequence").map_err(err)?;
    for row in q.query_map([previous],|x|Ok((x.get::<_,i64>(0)?,x.get::<_,String>(1)?))).map_err(err)? {let (seq,payload)=row.map_err(err)?;let e=val(payload)?;
        // Events have only protocol whitelisted payloads. No reveal/salt/upload tokens.
        let safe=json!({"source":"authority-protocol","sourceSequence":seq.to_string(),"kind":e["kind"],"roundId":e["roundId"],"createdAt":e["createdAt"]});
        tx.execute("INSERT OR IGNORE INTO explorer_events(source,source_sequence,payload) VALUES('authority-protocol',?,?)",params![seq.to_string(),safe.to_string()]).map_err(err)?;
    }
    let active=source.query_row("SELECT round_id FROM target_rounds WHERE campaign_id=? AND status IN ('OPEN','VERIFYING')",[campaign],|x|x.get::<_,String>(0)).optional().map_err(err)?;
    let current=active.as_ref().map(|rid|tx.query_row("SELECT document FROM explorer_objects WHERE object_type='round' AND id=?",[rid],|x|x.get::<_,String>(0)).map_err(err).and_then(val)).transpose()?;
    let pending:i64=source.query_row("SELECT COUNT(*) FROM goal_plan_progress WHERE campaign_id=? AND state='PENDING'",[campaign],|x|x.get(0)).map_err(err)?;
    let solved:i64=source.query_row("SELECT COUNT(*) FROM goal_plan_progress WHERE campaign_id=? AND state='SOLVED'",[campaign],|x|x.get(0)).map_err(err)?;
    let deferred:i64=source.query_row("SELECT COUNT(*) FROM goal_schedule WHERE campaign_id=? AND eligible_at>?",params![campaign,at],|x|x.get(0)).map_err(err)?;
    let queued=scalar(&source,"SELECT COUNT(*) FROM verification_jobs j JOIN candidate_claims c USING(claim_id) WHERE c.status='REVEALED' AND j.status IN ('PENDING','LEASED')")?;
    let active_miners=source.query_row("SELECT COUNT(DISTINCT miner_id) FROM target_sessions WHERE last_seen>? AND phase IN ('searching','computing')",[at-45],|x|x.get::<_,i64>(0)).map_err(err)?;
    let overview=json!({"researchGoal":{"zh":"果蝇神经网络刺激响应研究","en":"Fruit-fly neural network stimulus-response research"},"currentRound":current,"supply":{"remainingGoals":pending.to_string(),"solvedGoals":solved.to_string(),"deferredGoals":deferred.to_string(),"status":if pending==0{"published-plan-complete"}else if deferred>=pending{"cooldown"}else{"available"},"scope":"published finite plan; not all fruit-fly scientific questions","nextBatch":"admin-signed-real-calibration-required","pendingCalibration":null},"metrics":{"verifiedRecords":scalar(&tx,"SELECT COUNT(*) FROM explorer_objects WHERE object_type='record'")?.to_string(),"publicResults":scalar(&source,"SELECT COUNT(*) FROM public_results")?.to_string(),"indexedSimulations":scalar(&source,"SELECT COUNT(*) FROM science_simulations")?.to_string(),"activeMinerIdentities":active_miners.to_string(),"verificationQueue":queued.to_string(),"confirmedAtoms":null,"paidAtoms":null,"scope":"current authority; active identities over 45 seconds; no independent-operator inference","asOf":at},"chainStatus":if chains.is_empty(){"NOT_CONFIGURED"}else if chains.iter().all(|c|c["state"]=="indexed-project-scope"){"project-scope-indexed"}else{"syncing"},"chainNamespaces":chains.iter().map(|c|json!({"namespace":c["namespace"],"genesisHash":c["genesisHash"],"programId":c["configuration"]["programId"],"state":c["state"],"observedAt":c["updatedAt"]})).collect::<Vec<_>>(),"network":{"authorityId":authority,"networkId":network,"campaignId":campaign,"protocolVersion":2,"verificationKeys":3,"provenOperators":1,"independentOperators":null,"quorum":2,"verificationTrust":"authorized-full-replay; one operator controls three identities","model":current.as_ref().map(|c|c["descriptor"]["pre"]["modelRoot"].clone()),"selfReportedWork":"identity-signed-not-verified-work","legacyNamespaces":{"pow-v1":"source archive not configured; native protocol hash is not a Solana slot","campaign-v1":"requires explicit genesis/program and strict receipt/permanent account adapter","economic-v2":"actual configured chain namespace only"},"rpcHistoryCoverage":if chains.is_empty(){"NOT_CONFIGURED"}else{"per-chain-project-checkpoints; not whole-chain coverage"}}});
    for (k,v) in [("overview",overview.to_string()),("asOf",at.to_string()),("bootstrap","complete".into()),("indexVersion",INDEX_VERSION.into()),("sourceProtocol",protocol.to_string())]{tx.execute("INSERT INTO explorer_meta VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![k,v]).map_err(err)?;}
    for (name,seq) in [("authority-protocol",protocol),("product-audit",product),("economic-events",economy)]{tx.execute("INSERT INTO explorer_checkpoints VALUES(?,?,?) ON CONFLICT(source) DO UPDATE SET sequence=excluded.sequence,updated_at=excluded.updated_at",params![name,seq,at]).map_err(err)?;}
    let count=scalar(&tx,"SELECT COUNT(*) FROM explorer_objects")?;tx.commit().map_err(err)?;source.execute_batch("COMMIT").map_err(err)?;
    Ok(json!({"objects":count.to_string(),"indexedThrough":{"authorityProtocol":protocol.to_string(),"productAudit":product.to_string(),"economicEvents":economy.to_string()},"indexVersion":INDEX_VERSION,"at":at}))
}
pub fn rebuild(data:&Path,index:&Path,network:&str,authority:&str,campaign:&str)->Result<Value>{
    let temporary=index.with_extension(format!("rebuild-{}.sqlite",crate::target_authority::now()));if temporary.exists(){return Err("CONFLICT: rebuild path exists".into());}
    let report=project(data,&temporary,network,authority,campaign)?;let db=Connection::open(&temporary).map_err(err)?;db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").map_err(err)?;drop(db);
    // Kept side by side; operator switches only after comparison. Never replaces
    // a live reader's DB inode and never replays ledger effects.
    Ok(json!({"report":report,"rebuiltProjection":temporary,"sourceUntouched":true,"activate":"stop query service, preserve old index/WAL, switch validated file"}))
}
