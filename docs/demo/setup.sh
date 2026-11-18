#!/bin/sh
# Builds rpt-tui and writes the demo report, config and history folder the tapes record,
# under /tmp/rpt-demo; nothing else is touched. Needs rpt (with `rpt verbs`) on PATH.
# Run from the repo root: docs/demo/setup.sh, then vhs docs/demo/<name>.tape.
set -eu
cd "$(dirname "$0")/../.."
D=/tmp/rpt-demo
rm -rf "$D"
mkdir -p "$D/bin" "$D/config/rpt-tui" "$D/state"

cargo build --release --quiet
cp target/release/rpt-tui "$D/bin/"

R="$D/acme-external"
rpt new "$R"
cd "$R"
cat >> scope.csv <<'EOF'
S-01,app.acme.example,443,tcp
S-02,api.acme.example,443,tcp
S-03,10.20.0.0/24,,
EOF

# One record per line. F-005 lacks a remediation and F-006 names a scope row that doesn't
# exist, so the Problems panel has something to show.
rpt add > /dev/null <<'EOF'
{"kind":"finding","title":"Unauthenticated RCE in file upload","severity":"critical","scope":["S-01"],"sections":{"description":"The `/upload` endpoint stores files under the web root without checking their type, so an uploaded `.jsp` runs as the application user.","remediation":"Store uploads outside the web root and allow only the expected content types.","poc":"```sh\ncurl -F 'f=@shell.jsp' https://app.acme.example/upload\ncurl https://app.acme.example/files/shell.jsp?c=id\n```"}}
{"kind":"finding","title":"SQL injection in login","severity":"high","scope":["S-01"],"sections":{"description":"The `user` parameter of `/login` is concatenated into a SQL query.","remediation":"Use parameterised queries.","poc":"```sh\ncurl -d \"user=' OR 1=1--\" https://app.acme.example/login\n```"}}
{"kind":"finding","title":"Kerberoastable service accounts","severity":"high","scope":["S-03"],"sections":{"description":"Four service accounts have SPNs and passwords that crack within an hour.","remediation":"Use group managed service accounts, or 30+ character random passwords.","poc":"`GetUserSPNs.py acme.local/jdoe -request` returned four TGS hashes."}}
{"kind":"finding","title":"IDOR on invoice download","severity":"medium","scope":["S-02"],"sections":{"description":"`GET /invoices/{id}` returns any customer's invoice to any signed-in user.","remediation":"Check that the invoice belongs to the caller.","poc":"Changing the id from 1041 to 1040 returned another customer's invoice."}}
{"kind":"finding","title":"Reflected XSS in search","severity":"medium","scope":["S-01"],"sections":{"description":"The `q` parameter is reflected unescaped.","poc":"`/search?q=<script>alert(1)</script>`"}}
{"kind":"finding","title":"Weak TLS configuration","severity":"low","scope":["S-09"],"sections":{"description":"TLS 1.0 and CBC ciphers are enabled.","remediation":"Allow TLS 1.2 and 1.3 only.","poc":"`testssl.sh api.acme.example`"}}
{"kind":"finding","title":"Verbose server banner","severity":"info","scope":["S-02"],"sections":{"description":"Responses carry `Server: Apache/2.4.41 (Ubuntu)`.","remediation":"Drop the version from the banner.","poc":"`curl -I https://api.acme.example`"}}
EOF

cat > "$D/config/rpt-tui/config.yaml" <<'EOF'
chains:
  retest:
    description: Note a retest result on the selected finding
    key: R
    needs: finding
    params:
      RESULT: { kind: choice, options: [fixed, still-open, partly-fixed] }
    steps:
      - rpt set "$RPT_FINDING_ID" retest "$RESULT"
      - rpt findings
EOF
echo "demo ready: $R"
