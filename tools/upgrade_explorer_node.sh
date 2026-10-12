#!/usr/bin/env bash
# Upgrade the established authorized node. Preserve every private identity,
# source database, archived output and existing public URL.
set -euo pipefail
flypow_upload=${1:?validated upload directory required}
[[ "$flypow_upload" == /home/ubuntu/flypow-explorer-upload-* ]]
for flypow_binary in target_authority flypow_explorer_query flypow_explorer_indexer; do test -x "$flypow_upload/$flypow_binary"; done
test -f /var/lib/flypow/authority/private/admin.key
umask 077
flypow_backup=${2:-/var/backups/flypow/explorer-upgrade-$(date -u +%Y%m%dT%H%M%SZ)-$$}
[[ "$flypow_backup" == /var/backups/flypow/explorer-upgrade-* ]]
flypow_reuse_snapshot=0
if [ -n "${2:-}" ]; then
 # Only the same root-owned snapshot just validated by the preceding attempt.
 # Do not overwrite it. Rollback remains binary/config only, preserving all
 # newer source writes; this is not permission to restore an older database.
 test -f "$flypow_backup/before.tar.gz"; test -f "$flypow_backup/SHA256SUMS"
 test "$(stat -c %U "$flypow_backup/before.tar.gz")" = root
 flypow_reuse_snapshot=1
else mkdir -m 700 "$flypow_backup"; fi
flypow_query_was_active=0
if systemctl is-active --quiet flypow-explorer-query; then flypow_query_was_active=1; fi
systemctl stop flypow-target-verifier-serial
systemctl stop flypow-explorer-query 2>/dev/null || true
systemctl stop flypow-target-authority
flypow_recover(){
 systemctl stop flypow-explorer-query flypow-target-authority 2>/dev/null || true
 if test -f "$flypow_backup/target_authority.rollback"; then cp -a "$flypow_backup/target_authority.rollback" /opt/flypow/bin/target_authority; fi
 if test -f "$flypow_backup/nginx-product-api.rollback"; then cp -a "$flypow_backup/nginx-product-api.rollback" /opt/flypow/deploy/product-v2/nginx-product-api.conf; fi
 if [ "$flypow_query_was_active" = 0 ]; then rm -f /etc/nginx/conf.d/flypow-explorer-limits.conf; systemctl disable flypow-explorer-query 2>/dev/null || true; fi
 systemctl start flypow-target-authority flypow-target-verifier-serial
 if [ "$flypow_query_was_active" = 1 ]; then systemctl start flypow-explorer-query; fi
}
trap flypow_recover EXIT
if [ "$flypow_reuse_snapshot" = 0 ]; then
tar -czf "$flypow_backup/before.tar.gz" -C / var/lib/flypow/authority etc/flypow opt/flypow/bin opt/flypow/deploy etc/nginx/conf.d etc/systemd/system/flypow-target-authority.service etc/systemd/system/flypow-target-authority.service.d
gzip -t "$flypow_backup/before.tar.gz"
sha256sum "$flypow_backup/before.tar.gz" > "$flypow_backup/SHA256SUMS"
sha256sum -c "$flypow_backup/SHA256SUMS"
else printf "Reusing the just-verified consistent snapshot: %s\n" "$flypow_backup"; fi
cp -a /opt/flypow/bin/target_authority "$flypow_backup/target_authority.rollback"
cp -a /opt/flypow/deploy/product-v2/nginx-product-api.conf "$flypow_backup/nginx-product-api.rollback"
for flypow_binary in target_authority flypow_explorer_query flypow_explorer_indexer; do
 install -o root -g root -m 755 "$flypow_upload/$flypow_binary" "/opt/flypow/bin/$flypow_binary.new"
 mv "/opt/flypow/bin/$flypow_binary.new" "/opt/flypow/bin/$flypow_binary"
done
install -o root -g root -m 644 "$flypow_upload/nginx-product-api.conf" /opt/flypow/deploy/product-v2/nginx-product-api.conf
install -o root -g root -m 644 "$flypow_upload/nginx-http-limits.conf" /etc/nginx/conf.d/flypow-explorer-limits.conf
install -o root -g root -m 644 "$flypow_upload/flypow-explorer-query.service" /etc/systemd/system/flypow-explorer-query.service
if ! nginx -t; then
 cp -a "$flypow_backup/target_authority.rollback" /opt/flypow/bin/target_authority
 cp -a "$flypow_backup/nginx-product-api.rollback" /opt/flypow/deploy/product-v2/nginx-product-api.conf
 rm -f /etc/nginx/conf.d/flypow-explorer-limits.conf
 exit 1
fi
systemctl daemon-reload
systemctl start flypow-target-authority
# Startup performs the checked public-object migration, response-role backfill
# and initial projection before binding the authority listener.
for flypow_wait in $(seq 1 90); do
 if curl -fsS --max-time 2 http://127.0.0.1:8790/api/target-mining/v2/readiness >/dev/null; then break; fi
 sleep 1
done
curl -fsS --max-time 5 http://127.0.0.1:8790/api/target-mining/v2/readiness >/dev/null
systemctl enable flypow-explorer-query
systemctl start flypow-explorer-query
for flypow_wait in $(seq 1 60); do
 if curl -fsS --max-time 2 http://127.0.0.1:8791/api/explorer/v1/overview >/dev/null 2>&1; then break; fi
 sleep 1
done
curl -fsS --max-time 10 http://127.0.0.1:8791/api/explorer/v1/overview >/dev/null
systemctl start flypow-target-verifier-serial
systemctl reload nginx
trap - EXIT
systemctl is-active flypow-target-authority flypow-target-verifier-serial flypow-explorer-query
sha256sum /opt/flypow/bin/target_authority /opt/flypow/bin/flypow_explorer_query /opt/flypow/bin/flypow_explorer_indexer
printf 'Verified consistent backup: %s\n' "$flypow_backup"
