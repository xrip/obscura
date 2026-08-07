#!/usr/bin/env bash
# Fetch the DB-IP City Lite GeoIP database and unpack it to geoip.mmdb.
#
# DB-IP publishes a fresh database once a month at a date-stamped URL:
#   https://download.db-ip.com/free/dbip-city-lite-YYYY-MM.mmdb.gz
# It is free to download and use without a license key. The database is
# git-ignored (see .gitignore); this script is the build-time source of it.
#
# Dependencies: curl, gzip (gunzip). On failure the script exits non-zero and
# leaves any pre-existing geoip.mmdb untouched.
#
# Usage:
#   tools/fetch-geoip.sh [OUTDIR]     # default OUTDIR = repo root
set -euo pipefail

BASE_URL="https://download.db-ip.com/free"

outdir="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
outfile="$outdir/geoip.mmdb"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

# Candidates: this month, then the previous month (this month's file is not
# published until a few days into the month).
candidates=()
for m in 0 1; do
    ym="$(date -u -d "now - ${m} months" +"%Y-%m" 2>/dev/null || echo "")"
    [ -n "$ym" ] && candidates+=("dbip-city-lite-$ym.mmdb.gz")
done
# Portable fallback for macOS (no GNU date -d): this month only.
if [ ${#candidates[@]} -eq 0 ]; then
    candidates=("dbip-city-lite-$(date -u +"%Y-%m").mmdb.gz")
fi

downloaded=""
for name in "${candidates[@]}"; do
    [ -z "$name" ] && continue
    url="$BASE_URL/$name"
    echo "fetch-geoip: trying $url"
    if curl -fsSL --retry 3 "$url" -o "$tmpdir/db.gz"; then
        downloaded=1
        break
    fi
done

if [ -z "$downloaded" ]; then
    echo "fetch-geoip: ERROR - could not download a DB-IP database (tried: ${candidates[*]})" >&2
    exit 1
fi

gunzip -c "$tmpdir/db.gz" > "$outfile.tmp"
mv "$outfile.tmp" "$outfile"
echo "fetch-geoip: wrote $outfile ($(du -h "$outfile" | cut -f1))"