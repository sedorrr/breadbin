#!/usr/bin/env bash
# macOS-Signatur mit eigenem (auch selbstsigniertem) Zertifikat einrichten – für pakete.yml und signatur-test.yml.
# Erwartet CERT (p12, base64), CERT_PW, CERT_ID (Name der Identität). Ohne CERT/CERT_ID: nichts tun (ad hoc).
# Legt einen eigenen Schlüsselbund an, importiert Zertifikat + Schlüssel, erlaubt codesign den Zugriff ohne Rückfrage
# und vertraut dem Zertifikat fürs Signieren (sonst zählt „security find-identity -v“ es nicht als gültig).
set -euo pipefail
if [ -z "${CERT:-}" ] || [ -z "${CERT_ID:-}" ]; then
  echo "Kein Zertifikat hinterlegt – ad hoc signiert."
  exit 0
fi
KC="$RUNNER_TEMP/breadbin.keychain-db"
KP="$(openssl rand -hex 16)"
P12="$RUNNER_TEMP/zert.p12"
PEM="$RUNNER_TEMP/zert.pem"
echo "$CERT" | base64 --decode > "$P12"
security create-keychain -p "$KP" "$KC"
security set-keychain-settings -lut 21600 "$KC"
security unlock-keychain -p "$KP" "$KC"
security import "$P12" -k "$KC" -P "$CERT_PW" -T /usr/bin/codesign -T /usr/bin/security >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KP" "$KC" >/dev/null
# eigenen Schlüsselbund vorn in die Suchliste
security list-keychains -d user -s "$KC" $(security list-keychains -d user | tr -d '"')
openssl pkcs12 -in "$P12" -nokeys -clcerts -passin env:CERT_PW -out "$PEM" 2>/dev/null \
  || openssl pkcs12 -legacy -in "$P12" -nokeys -clcerts -passin env:CERT_PW -out "$PEM"
sudo security add-trusted-cert -d -r trustRoot -p codeSign -k /Library/Keychains/System.keychain "$PEM"
echo "Zertifikat: $(openssl x509 -in "$PEM" -noout -dates -ext extendedKeyUsage 2>/dev/null | tr '\n' ' ')"
rm -f "$P12" "$PEM"
echo "Alle Identitäten fürs Signieren:"
security find-identity -p codesigning "$KC" || true
echo "Davon gültig:"
security find-identity -v -p codesigning "$KC" || true
if [ -n "${GITHUB_ENV:-}" ] && [ "${SETZE_ENV:-0}" = 1 ]; then
  echo "APPLE_SIGNING_IDENTITY=$CERT_ID" >> "$GITHUB_ENV"
fi
echo "Signiere mit eigenem Zertifikat."
