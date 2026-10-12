#!/usr/bin/env python3
"""Record fingerprints, never private row contents or key hashes in output."""
import json,sqlite3,hashlib,urllib.request
from pathlib import Path
root=Path('/var/lib/flypow/authority');db=sqlite3.connect('file:'+str(root/'target-v2.sqlite')+'?mode=ro',uri=True)
def digest(query):
 return hashlib.sha256(json.dumps(db.execute(query).fetchall(),sort_keys=True,separators=(',',':')).encode()).hexdigest()
report={'schema':'explorer-node-preservation-v1','winnerCount':db.execute('SELECT COUNT(*) FROM round_winners').fetchone()[0],'winnersRoot':digest('SELECT round_id,claim_id,winner_json FROM round_winners ORDER BY round_id'),'publicFilesRoot':digest('SELECT result_id,name,sha256,bytes FROM public_result_files ORDER BY result_id,name'),'publicResults':db.execute('SELECT COUNT(*) FROM public_results').fetchone()[0],'privateRequestsRoot':digest('SELECT id,fields,status,access_hash FROM service_requests ORDER BY id'),'quotesRoot':digest('SELECT id,request_id,asset,amount_atoms,expires,terms,confirmed_by FROM economic_quotes ORDER BY id'),'economicBooks':db.execute('SELECT COUNT(*) FROM economic_books').fetchone()[0],'sealedArchiveCount':db.execute("SELECT COUNT(*) FROM candidate_artifacts WHERE status='SEALED'").fetchone()[0],'sealedRoots':digest("SELECT upload_id,sha256,bytes FROM candidate_artifacts WHERE status='SEALED' ORDER BY upload_id")}
req=urllib.request.Request('http://127.0.0.1:8790/api/target-mining/v2/readiness');ready=json.load(urllib.request.urlopen(req));report.update(authorityId=ready['authorityId'],modelIdentity=ready['modelIdentity'],payment=ready['payment'],rewards=ready['rewards']);print(json.dumps(report,separators=(',',':')))
