"""Actual pages, retained caches, no mining or artifact creation on public visits."""
import argparse,json,time
from pathlib import Path
from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.support.ui import WebDriverWait
from browser_cache import retained_profile,close_test_browser
from e2e_official_download import BrowserNetwork
p=argparse.ArgumentParser();p.add_argument('--site',required=True);p.add_argument('--report',required=True);modes=p.add_mutually_exclusive_group();modes.add_argument('--mining-only',action='store_true');modes.add_argument('--explorer-only',action='store_true');a=p.parse_args()
out=Path(a.report);out.parent.mkdir(parents=True,exist_ok=True);report={'schema':'explorer-mining-stage-browser-v1','site':a.site,'retainedProfile':True,'physicalAndroid':False,'physicalGPU':False,'checks':[],'screenshots':[]}
def check(label):report['checks'].append(label);out.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'passed':len(report['checks']),'check':label}),flush=True)
with retained_profile() as profile:
 o=webdriver.ChromeOptions();o.binary_location='/usr/bin/chromium'
 for arg in ['--headless=new','--no-sandbox','--disable-dev-shm-usage','--no-proxy-server','--enable-unsafe-swiftshader','--use-gl=angle','--use-angle=swiftshader','--user-data-dir='+profile,'--window-size=1440,1050']:o.add_argument(arg)
 o.set_capability('goog:loggingPrefs',{'browser':'ALL'});d=webdriver.Chrome(options=o);d.set_script_timeout(300);net=BrowserNetwork(d);js=d.execute_script;wait=WebDriverWait(d,120,poll_frequency=.25)
 def snap():return js('return window.flypowWorkspace?.snapshot()')
 def asyncjs(script):return d.execute_async_script('const done=arguments[arguments.length-1];(async()=>{'+script+'})().then(done,e=>done({error:e.message}));')
 def shot(name):path=out.parent/(name+'.png');d.save_screenshot(str(path));report['screenshots'].append(str(path))
 def width(w):d.execute_cdp_cmd('Emulation.setDeviceMetricsOverride',{'width':w,'height':1050,'deviceScaleFactor':1,'mobile':False})
 def visible():return js("return [...document.querySelectorAll('[role=tabpanel][id^=miner-stage-]')].filter(e=>!e.hidden).map(e=>e.id)")
 try:
  if not a.explorer_only:
   d.get(a.site.rstrip('/')+'/mining/');wait.until(lambda _:snap() and snap()['preparation']!='checking');assert not snap()['running'] and not snap()['ready'];check('Opening mining starts neither model preparation nor computation')
   # These screenshots use the real cached/empty inventory, not injected phases.
   expected='build' if snap()['cacheModel'] else 'download';assert visible()==['miner-stage-'+expected];check('Only the actual cache-dependent preparation stage is expanded')
   for lang in ['zh','en']:
    js('FlyI18n.setLanguage(arguments[0])',lang)
    for w in [390,1440]:
     width(w);time.sleep(.3);assert js('return document.documentElement.scrollWidth<=innerWidth+1');tabs=js("return [...document.querySelectorAll('.miner-step-tabs [role=tab]')].map(e=>({selected:e.getAttribute('aria-selected'),text:e.innerText,top:e.getBoundingClientRect().top}))");assert len(tabs)==3 and sum(t['selected']=='true' for t in tabs)==1;assert max(t['top'] for t in tabs)-min(t['top'] for t in tabs)<1;shot('mining-stage-'+lang+'-'+str(w))
   check('390px and desktop stages remain in one row, translated, with no horizontal overflow')
   if snap()['cacheModel']:
    width(1440);js("document.getElementById('miner-graphics').checked=false;document.getElementById('miner-graphics').dispatchEvent(new Event('change'))");js("document.getElementById('miner-primary').click()");wait.until(lambda _:snap()['ready'] and not snap()['preparationBusy']);assert not snap()['running'];assert visible()==['miner-stage-mining'];assert not js("return document.getElementById('miner-live-progress').hidden");shot('mining-ready');check('Real retained model integrity/load finishes at stage three and waits for a separate mining click')
   report['officialDownload']={'officialRequests':len(net.requests),'officialReceivedBytes':net.data_received_bytes};assert report['officialDownload']['officialRequests']==0;check('All official model caches retained; no official model request during this acceptance')
  if not a.mining_only:
   for route in ['/explorer/','/explorer/records/','/explorer/rounds/','/explorer/network/']:
    d.get(a.site.rstrip('/')+route);wait.until(lambda _:js("return document.getElementById('explorer-content')?.textContent.length>30"));assert 'Query unavailable.' not in js("return document.getElementById('explorer-content').textContent")
   check('Live Explorer overview, records, rounds and network pages load')
   d.get(a.site.rstrip('/')+'/explorer/records/');wait.until(lambda _:js("return !!document.querySelector('#explorer-content a[href^=\"/explorer/records/\"]')"));record=js("return document.querySelector('#explorer-content a[href^=\"/explorer/records/\"]').getAttribute('href')")
   for tab in ['calculation','competition','settlement','evidence']:
    d.get(a.site.rstrip('/')+record+'?tab='+tab);wait.until(lambda _:js("return document.getElementById('explorer-content').textContent.includes('Computation record')"));assert 'Query unavailable.' not in js("return document.getElementById('explorer-content').textContent")
   js("[...document.querySelectorAll('#explorer-content button')].find(e=>e.innerText.includes('Verify signatures')).click()");wait.until(lambda _:js("return document.getElementById('explorer-content').textContent.includes('Source signature verified')"));assert 'Signature rejected' not in js("return document.getElementById('explorer-content').textContent");check('Actual record detail tabs and source signatures verify in the browser')
   d.execute_cdp_cmd('Browser.grantPermissions',{'origin':a.site.rstrip('/'),'permissions':['clipboardReadWrite','clipboardSanitizedWrite']});copy=d.find_element(By.CSS_SELECTOR,'.explorer-copy');copy.click();wait.until(lambda _:copy.text=='Copied');check('Copy button survives asynchronous clipboard completion for a real public record')
   for lang in ['zh','en']:
    js('FlyI18n.setLanguage(arguments[0])',lang)
    for w in [390,1440]:
     width(w);time.sleep(.3);assert js('return document.documentElement.scrollWidth<=innerWidth+1');shot('explorer-evidence-'+lang+'-'+str(w))
   check('Explorer record evidence translates and fits both widths')
   width(1440);d.get(a.site.rstrip('/')+'/explorer/?q='+record.split('/')[3]);wait.until(lambda _:js("return document.getElementById('explorer-content').textContent.includes('Search results')"));assert js("return [...document.querySelectorAll('#explorer-content a')].some(a=>a.getAttribute('href')===arguments[0])",record);d.back();d.forward();wait.until(lambda _:js("return document.getElementById('explorer-content').textContent.includes('Search results')"));check('Typed record search and browser back/forward preserve deep routes')
   d.get(a.site.rstrip('/')+'/explorer/records/'+'f'*64+'/');wait.until(lambda _:js("return document.title.startsWith('404')"));assert js("return document.getElementById('explorer-content').textContent.includes('Object not found')");check('Unknown record returns an explicit 404 state')
   d.get(a.site.rstrip('/')+'/explorer/chains/unconfigured/blocks/100/');wait.until(lambda _:js("return document.getElementById('explorer-content').textContent.includes('Chain identity is not configured')"));check('Unconfigured chain shows capability state rather than a fabricated block')
   d.get(a.site.rstrip('/')+'/mining/');wait.until(lambda _:snap() and snap()['preparation']!='checking');assert not snap()['running'] and not snap()['ready'];assert not d.find_elements(By.ID,'miner-network-badge');assert not d.find_elements(By.CSS_SELECTOR,'.console-heading');assert len(visible())==1;check('Final mining page renders one real stage without title/network badge or automatic CPU work')
  report['browserErrors']=[r['message'][:250] for r in d.get_log('browser') if r['level']=='SEVERE' and '404' not in r['message'] and '503' not in r['message']];report['passed']=True
 finally:
  try:report['finalUrl']=d.current_url;report['browserErrors']=[r['message'][:300] for r in d.get_log('browser') if r['level']=='SEVERE' and '404' not in r['message'] and '503' not in r['message']]
  except Exception:pass
  out.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');close_test_browser(d)
