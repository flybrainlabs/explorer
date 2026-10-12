#!/usr/bin/env python3
"""Generate the native Explorer shell; dynamic queries are public and read-only."""
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
BODY='''<div class="section-wrap explorer-page"><div class="explorer-toolbar"><form id="explorer-search" role="search"><input name="q" aria-label="Search records, rounds, addresses or signatures" placeholder="Search records, rounds, addresses or signatures" maxlength="256"><button class="button" type="submit">Search</button></form></div><nav class="explorer-nav" aria-label="Explorer navigation"><a href="/explorer/">Overview</a><a href="/explorer/records/">Computation records</a><a href="/explorer/rounds/">Computation rounds</a><a href="/explorer/network/">Network and index status</a><a href="/results/">Scientific results</a><a href="/account/">My records and payment settings</a></nav><div class="explorer-update" id="explorer-updated" aria-live="polite"></div><button class="explorer-refresh" id="explorer-new" type="button" hidden>New records available. Refresh this view.</button><div id="explorer-content" aria-live="polite"><div class="explorer-skeleton">Loading verified project records…</div></div></div><script type="module" src="/assets/explorer-app.mjs?v=explorer-20261012-r1"></script>'''
def build():
 (ROOT/'web/pages/explorer.html').write_text(BODY+'\n')
if __name__=='__main__':build()
