"use strict";
(async()=>{
 await FlyI18n.ready;
 const {api,format,downloadJSON}=FlySite,$=id=>document.getElementById(id),t=(s,v)=>FlyI18n.t(s,v);
 const time=timestamp=>new Date(Number(timestamp)*1000).toLocaleString(FlyI18n.language==="zh"?"zh-CN":"en-US",{hour12:false});
 const short=hash=>hash.slice(0,13)+"…"+hash.slice(-9);
 let page=1,query="",latest=null,lastProof=null,lastCheck=null;
 const hash=location.pathname.split("/")[2];
 if(hash){await loadDetail(hash);document.addEventListener("flypow:language",()=>loadDetail(hash));return;}
 async function load(){
  try{
   const result=await api("/api/blocks?page="+page+"&q="+encodeURIComponent(query));latest=result;page=result.page;
   $("block-total").textContent=format(result.total);$("block-attempts").textContent=format(result.total_attempts);$("block-latest").textContent=result.latest_timestamp?time(result.latest_timestamp):t("No records");
   $("blocks-empty").hidden=result.entries.length!==0;
   $("block-rows").replaceChildren(...result.entries.map(entry=>{
    const tr=document.createElement("tr");
    const values=["#"+entry.record,short(entry.hash),time(entry.timestamp),entry.difficulty_bits+" bit",entry.nonce,t("PoW generated"),"↗"];
    values.forEach((value,i)=>{const td=document.createElement("td");if(i===0||i===1||i===6){const a=document.createElement("a");a.href="/explorer/"+entry.hash;a.textContent=value;if(i===1)a.title=entry.hash;if(i===6)a.setAttribute("aria-label",t("Open candidate")+" "+entry.record);td.append(a);}else if(i===5){const span=document.createElement("span");span.className="state-pill";span.textContent=value;td.append(span);}else td.textContent=value;tr.append(td);});return tr;
   }));
   $("block-page-label").textContent=t("Records: {count} · Page {page}/{pages}",{count:format(result.matched),page:result.page,pages:result.pages});
   $("blocks-prev").disabled=page<=1;$("blocks-next").disabled=page>=result.pages;$("explorer-error").textContent="";
  }catch(error){$("explorer-error").textContent=t(error.message);}
 }
 $("block-search").onsubmit=event=>{event.preventDefault();query=$("block-query").value.trim();page=1;const p=new URLSearchParams();if(query)p.set("q",query);history.replaceState(null,"",location.pathname+(p.size?"?"+p:""));load();};
 $("blocks-prev").onclick=()=>{if(page>1){page--;load();}};$("blocks-next").onclick=()=>{if(latest&&page<latest.pages){page++;load();}};
 query=new URLSearchParams(location.search).get("q")||"";$("block-query").value=query;
 load();setInterval(()=>{if(!document.hidden)load();},5000);document.addEventListener("flypow:language",load);
 async function loadDetail(hash){
  if(!/^[0-9a-f]{64}$/.test(hash)){$("detail-error").textContent=t("Invalid work hash.");return;}
  try{
   const result=await api("/api/blocks/"+hash),block=result.block,proof=block.proof;lastProof=proof;
   $("block-detail-title").textContent=t("Candidate record")+" #"+result.record+".";
   $("detail-hash").textContent=proof.work_hash;
   const fields=[["Created",time(proof.template.timestamp)],["Block message",proof.message],["Difficulty",proof.template.difficulty_bits+" bit"],["Nonce",proof.nonce],["Attempts",format(block.attempts)],["Elapsed",(block.elapsed_ms/1000).toFixed(3)+" s"],["Access rounds",String(proof.access_rounds)],["Dataset root",proof.dataset_root],["Index root",proof.artifact_root]];
   $("detail-fields").replaceChildren(...fields.flatMap(([name,value])=>{const dt=document.createElement("dt"),dd=document.createElement("dd");dt.textContent=t(name);dd.textContent=value;dd.dataset.noI18n="";return [dt,dd];}));
   $("detail-proof").textContent=JSON.stringify(proof,null,2);$("detail-open-miner").href="/mining?proof="+hash+"#verification";
   $("detail-export").onclick=()=>downloadJSON(lastProof,"flypow-proof-"+lastProof.nonce+".json");
   $("detail-verify").onclick=async()=>{
    const button=$("detail-verify");button.disabled=true;$("detail-verification").textContent=t("Recomputing…");
    try{lastCheck=await api("/api/verify",lastProof);renderCheck();}catch(error){$("detail-verification").textContent=t(error.message);}finally{button.disabled=false;}
   };
   $("block-detail").hidden=false;renderCheck();
  }catch(error){$("detail-error").textContent=t(error.message);}
 }
 function renderCheck(){
  if(!lastCheck)return;
  $("detail-verification").textContent=(lastCheck.valid?"✓ "+t("Verification passed"):"× "+t("Verification failed"))+" · "+lastCheck.checks.length+" "+t("checks")+" · "+lastCheck.elapsed_ms.toFixed(2)+" ms";
 }
})();
