export const short=(s,n=10)=>typeof s==='string'&&s.length>n*2?s.slice(0,n)+'…'+s.slice(-n):String(s??'—');
export function atoms(value,decimals=8){if(value==null)return '—';const n=BigInt(value),base=10n**BigInt(decimals);return (n/base).toLocaleString('en-US')+'.'+(n%base).toString().padStart(decimals,'0');}
export function time(value){return value==null?'—':new Date(Number(value)*1000).toLocaleString(globalThis.FlyI18n?.language==='zh'?'zh-CN':'en');}
export function url(kind,id='',namespace='target-v2'){
 const e=encodeURIComponent;
 if(['record','round','candidate'].includes(kind))return '/explorer/'+({record:'records',round:'rounds',candidate:'candidates'}[kind])+(id?'/'+e(id):'')+'/';
 if(kind==='result'||kind==='goal')return '/'+(kind==='result'?'results':'tasks')+'/'+e(id)+'/';
 if(kind==='body')return '/atlas/?body='+e(id);
 if(kind==='miner')return '/explorer/protocols/target-v2/miners/'+e(id)+'/';
 if(['tx','block','address','stage','right','receipt'].includes(kind)){if(!namespace||namespace==='target-v2')throw new Error('Stable chain namespace required');return '/explorer/chains/'+e(namespace)+'/'+({tx:'tx',block:'blocks',address:'address',stage:'stages',right:'rights',receipt:'receipts'}[kind])+'/'+e(id)+'/';}
 throw new Error('Unknown Explorer object type');
}
export const translated=v=>typeof v==='object'&&v?v[globalThis.FlyI18n?.language]||v.en||'—':String(v??'—');
