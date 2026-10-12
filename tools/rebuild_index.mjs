// Reconstruct a disposable public index. Every entitlement is read from program-owned
// permanent accounts; this file can never authorize settlement or replace on-chain state.
import { readFile,writeFile } from 'node:fs/promises';
import { CampaignClient } from '../web/solana-client.mjs';
import { hex } from '../web/campaign-codec.mjs';
import { loadArchivedResponse } from '../web/response-store.mjs';
const [chainPath,researchPath,out,...flags]=process.argv.slice(2);
if(!out)throw new Error('Usage: node tools/rebuild_index.mjs PUBLIC_CHAIN PUBLIC_RESEARCH NEW_INDEX.json [--check-previews]');
const client=new CampaignClient(JSON.parse(await readFile(chainPath))),config=JSON.parse(await readFile(researchPath)),snapshot=await client.snapshot();
const responses=[];
for(const page of snapshot.pages)if(page.complete){
  const entitlement=snapshot.entitlements.find(e=>hex(e.submission)===hex(page.winner));
  const response={task_id:hex(page.taskId),k:page.k,body_id:page.task.bodyId,archived:page.archived,result_root:hex(page.resultRoot),preview_root:hex(page.previewRoot),archive_manifest_hash:hex(page.archiveRoot),summary_hash:hex(page.summaryHash),winner:page.winner,entitlement,archive_availability:'UNCHECKED'};
  if(flags.includes('--check-previews')&&page.archived)try{const bundle=await loadArchivedResponse(client,page,entitlement,config);response.record=bundle.record;response.archive_availability='CHECKED_NOW';}catch(error){response.archive_availability='UNAVAILABLE';response.error=error.message;}
  responses.push(response);
}
function normalize(v){if(typeof v==='bigint')return v.toString();if(v?.toBase58)return v.toBase58();if(v instanceof Uint8Array)return hex(v);if(Array.isArray(v))return v.map(normalize);if(v&&typeof v==='object')return Object.fromEntries(Object.entries(v).map(([k,x])=>[k,normalize(x)]));return v;}
const report={schema:'flypow-rebuildable-chain-index-v1',authoritative:false,source:{program:client.program,campaign:client.campaign,genesis:client.config.genesis},observed_at:new Date().toISOString(),state:snapshot.state.state,epochs:snapshot.epochs,entitlements:snapshot.entitlements,responses};
await writeFile(out,JSON.stringify(normalize(report),null,2)+'\n',{flag:'wx'});console.log({file:out,accepted:responses.length,archived:responses.filter(r=>r.archived).length});
