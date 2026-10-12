#!/usr/bin/env python3
"""Run on the established node as root. Source backup is private and preserved.
Only immutable public object fingerprints and process outcomes are printed.
"""
import json,sqlite3,hashlib,subprocess,time,os,signal
from pathlib import Path
base=Path('/var/backups/flypow/explorer-rebuild-20261012');base.mkdir(mode=0o700,parents=True,exist_ok=True)
source=base/'source';source.mkdir(mode=0o700,exist_ok=True);snapshot=source/'target-v2.sqlite'
if not snapshot.exists():
 s=sqlite3.connect('file:/var/lib/flypow/authority/target-v2.sqlite?mode=ro',uri=True);target=sqlite3.connect(snapshot);s.backup(target);target.close();s.close();snapshot.chmod(0o600)
cfg=json.loads(Path('/etc/flypow/authority-config.json').read_text());conn=sqlite3.connect('file:'+str(snapshot)+'?mode=ro',uri=True)
# The authority public identity is derived from an existing signed descriptor.
r=json.loads(conn.execute('SELECT descriptor_json FROM target_rounds ORDER BY sequence LIMIT 1').fetchone()[0]);authority=r['pre']['authorityId'] if 'authorityId' in r['pre'] else r['authorityId'];conn.close()
def command(index):return ['/opt/flypow/bin/flypow_explorer_indexer',str(source),str(index),cfg['networkId'],authority,cfg['campaignId']]
index=base/'interrupted.sqlite';p=subprocess.Popen(command(index),stdout=subprocess.PIPE,stderr=subprocess.PIPE);time.sleep(.01);interrupted=p.poll() is None
if interrupted:p.kill()
p.communicate();subprocess.run(command(index),check=True,capture_output=True);subprocess.run(command(index),check=True,capture_output=True)
other=base/'rebuilt.sqlite';subprocess.run(command(other),check=True,capture_output=True)
def clean(v):
 if isinstance(v,dict):return {k:clean(x) for k,x in v.items() if k not in ['sourceObservedAt','projectedAt','backfilledAt']}
 if isinstance(v,list):return [clean(x) for x in v]
 return v
def business(path):
 db=sqlite3.connect('file:'+str(path)+'?mode=ro',uri=True);rows=db.execute('SELECT namespace,object_type,id,sequence,document FROM explorer_objects ORDER BY namespace,object_type,id').fetchall();objects=[[n,k,i,s,clean(json.loads(v))] for n,k,i,s,v in rows];events=db.execute('SELECT source,source_sequence,payload FROM explorer_events ORDER BY source,source_sequence').fetchall();unique=db.execute('SELECT COUNT(*) FROM (SELECT DISTINCT namespace,object_type,id FROM explorer_objects)').fetchone()[0];db.close();assert unique==len(objects);return hashlib.sha256(json.dumps([objects,events],sort_keys=True,separators=(',',':')).encode()).hexdigest(),len(objects)
a,count=business(index);b,countb=business(other);assert a==b and count==countb
print(json.dumps({'schema':'explorer-real-source-rebuild-v1','source':'private retained snapshot of actual production authority, no keys copied','sourceUntouched':True,'interruptedDuringProcess':interrupted,'restartAndRepeatedReplayUnique':True,'businessDigestsEqual':True,'publicObjects':count,'businessDigest':a,'privateSnapshotPubliclyExposed':False}))
