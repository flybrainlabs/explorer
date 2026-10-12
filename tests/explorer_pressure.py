"""Isolated 100k metadata reader pressure plus real native full-model replay.
No production request is made. Idle SSE sockets and active queries are separate.
"""
import asyncio,json,time,subprocess,statistics,os,signal
from pathlib import Path
from urllib.request import urlopen
ROOT=Path(__file__).resolve().parents[1];REPORT=ROOT/'data/prepared/explorer-20261012/pressure.json';PKG=ROOT/'data/prepared/product-site-20261011/public-package'
async def request(port,path,body=None):
 r,w=await asyncio.open_connection('127.0.0.1',port);method='POST' if body else 'GET';blob=b'' if body is None else body;w.write(f'{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len(blob)}\r\nConnection: close\r\n\r\n'.encode()+blob);await w.drain();data=await r.read();w.close();await w.wait_closed();assert b'200 OK' in data.split(b'\r\n',1)[0],data[:100];return data
async def samples(port,path,n=30,body=None):
 times=[]
 for _ in range(n):
  start=time.perf_counter();await request(port,path,body);times.append((time.perf_counter()-start)*1000)
 return {'p50Ms':statistics.median(times),'p95Ms':sorted(times)[int(.95*(len(times)-1))],'samples':len(times)}
async def native():
 p=await asyncio.create_subprocess_exec(str(ROOT/'target/release/target_trial'),str(PKG/'graph.bin'),str(PKG/'model.bin'),str(PKG/'goal.json'),str(PKG/'input.json'),'74ae3c5e928f289c',str(PKG/'trial-0.bin'),'--verify',stdout=asyncio.subprocess.PIPE,stderr=asyncio.subprocess.PIPE);start=time.perf_counter();o,e=await p.communicate();assert p.returncode==0,e[:300];v=json.loads(o);return {'elapsedMs':(time.perf_counter()-start)*1000,'resultRoot':v['resultRoot'],'steps':256,'neurons':165122,'fullModel':True,'existingResultVerified':True}
async def reader_cpu_budget(pid,stop):
 # Container cgroup is read-only. Limit this owned test reader to an average
 # 50 ms CPU / 100 ms period, matching production CPUQuota=50%, with signals.
 # This is a test limiter, not a claim of a production kernel cgroup here.
 ticks=os.sysconf('SC_CLK_TCK')
 def cpu():
  f=Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split();return (int(f[11])+int(f[12]))/ticks
 try:
  while not stop.is_set():
   begin=time.perf_counter();start=cpu();os.kill(pid,signal.SIGCONT)
   while time.perf_counter()-begin<.1 and not stop.is_set():
    if cpu()-start>=.05:os.kill(pid,signal.SIGSTOP);break
    await asyncio.sleep(.005)
   await asyncio.sleep(max(0,.1-(time.perf_counter()-begin)))
 finally:os.kill(pid,signal.SIGCONT)
async def main():
 pid=next(int(e.name) for e in Path('/proc').iterdir() if e.name.isdigit() and (e/'cmdline').is_file() and (e/'cmdline').read_bytes().startswith(b'target/release/flypow_explorer_query\0data/prepared/explorer-20261012/volume.sqlite\0'));limiter_stop=asyncio.Event();limiter=asyncio.create_task(reader_cpu_budget(pid,limiter_stop))
 report={'readerResourceLimit':'owned reader 50ms CPU per 100ms period by test signals; production kernel CPUQuota=50%', 'schema':'explorer-isolated-pressure-v1','environment':'2 CPU / 2 GiB container; same host processes, not production','metadataRecords':100000,'syntheticMetadata':True,'nativeModelIsReal':True,'samples':[]}
 commit=Path('/workspace/.flypow-tools/explorer-pressure-20261012/commit-request.json').read_bytes();commit_baseline=await samples(8810,'/api/target-mining/v2/claims',body=commit)
 baseline=await samples(8810,'/api/target-mining/v2/readiness');baseline_native=await native();report['baseline']={'authorityReadiness':baseline,'idempotentCommitRetry':commit_baseline,'native':baseline_native};REPORT.write_text(json.dumps(report,indent=2)+'\n')
 for count in [100,1000]:
  sockets=[];sem=asyncio.Semaphore(50)
  async def connect():
   async with sem:
    r,w=await asyncio.open_connection('127.0.0.1',8811);w.write(b'GET /api/explorer/v1/stream?after=0 HTTP/1.1\r\nHost: localhost\r\n\r\n');await w.drain();await asyncio.wait_for(r.readuntil(b'\r\n\r\n'),20);sockets.append((r,w))
  start=time.perf_counter();await asyncio.gather(*(connect() for _ in range(count)));connected=(time.perf_counter()-start)*1000
  stop=asyncio.Event();times=[]
  async def queries():
   while not stop.is_set():
    start=time.perf_counter();await request(8811,'/api/explorer/v1/records?limit=20&goalId='+'a'*64);times.append((time.perf_counter()-start)*1000)
  clients=[asyncio.create_task(queries()) for _ in range(10)]
  authority=await samples(8810,'/api/target-mining/v2/readiness');commits=await samples(8810,'/api/target-mining/v2/claims',body=commit);model=await native();stop.set();await asyncio.gather(*clients)
  assert model['resultRoot']==baseline_native['resultRoot'];p95=sorted(times)[int(.95*(len(times)-1))];ratio=model['elapsedMs']/baseline_native['elapsedMs']
  item={'simultaneousSseConnections':count,'concurrentActiveQueryClients':10,'connectedMs':connected,'activeQueries':len(times),'queryP95Ms':p95,'authorityReadiness':authority,'idempotentCommitRetry':commits,'native':model,'nativeLatencyRatio':ratio,'queryServiceIsSeparate':True};report['samples'].append(item);REPORT.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in item.items() if k not in ['native']}),flush=True)
  for r,w in sockets:w.close()
  await asyncio.gather(*(w.wait_closed() for r,w in sockets),return_exceptions=True)
 limiter_stop.set();await limiter;report['scientificByteEquality']=True;report['measuredOnly']='No physical GPU, no production flood, no actual economic payment; Commit benchmark retries the same real signed isolated commitment; it is not a new-candidate throughput benchmark';REPORT.write_text(json.dumps(report,indent=2)+'\n')
asyncio.run(main())
