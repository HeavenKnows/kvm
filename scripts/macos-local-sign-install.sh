#!/bin/bash
# Stable personal signing: this installer runs on the Mac as the login user.
# NEVER run via sudo (the private key stays in your own login keychain).
set -euo pipefail
umask 077
NAME="kvm Local Code Signing"
ID="dev.heavenknows.kvm"
DEST="/Applications/kvm.app"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP=""
MOUNTED=0
err(){ echo "kvm: $*" >&2; exit 1; }
say(){ echo "kvm: $*"; }
clean(){
  if [[ "$MOUNTED" == 1 ]];then hdiutil detach "$TMP/mount" -quiet >/dev/null 2>&1||true;fi
  if [[ -n "$TMP" && -d "$TMP" ]];then rm -rf "$TMP";fi
}
trap clean EXIT
[[ "$(uname)" == Darwin ]]||err "Only supported on macOS."
[[ "$(id -u)" != 0 ]]||err "Run without sudo."
hash(){
  security find-identity -v -p codesigning 2>/dev/null|awk -F'"' -v cn="$NAME" \
    '$2==cn{n=$1;sub(/^[[:space:]]+[0-9]+\)[[:space:]]+/,"",n);gsub(/[[:space:]]/,"",n);print n;exit}'
}
work(){ if [[ -z "$TMP" ]];then TMP="$(mktemp -d "${TMPDIR:-/tmp/}kvm-sign.XXXXXXXX")";fi; }
setup(){
  local h chain password
  h="$(hash)"
  if [[ -n "$h" ]];then printf '%s\n' "$h";return;fi
  if security find-certificate -c "$NAME" >/dev/null 2>&1;then
    err "Previous local cert is invalid; repair it in Keychain Access, do not rotate it."
  fi
  work
  # CI may provide a temporary user keychain explicitly. Ordinary Macs
  # continue using their current default login keychain.
  chain="${KVM_KEYCHAIN_PATH:-$(security default-keychain -d user|tr -d '"')}"
  # Keychains can be managed by securityd and need not appear to this
  # shell as a regular file; let "security import" validate the keychain.
  [[ -n "$chain" ]]||err "No default user keychain was selected."
  password="$(openssl rand -hex 24)"
  cat >"$TMP/cert.conf" <<'CONF'
[req]
prompt=no
distinguished_name=subject
x509_extensions=codesign
[subject]
CN=kvm Local Code Signing
[codesign]
basicConstraints=critical,CA:TRUE
keyUsage=critical,digitalSignature,keyCertSign
extendedKeyUsage=critical,codeSigning
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid:always,issuer
CONF
  say "Generating one reusable signing certificate."
  openssl req -new -x509 -newkey rsa:3072 -nodes -sha256 -days 3650 \
    -config "$TMP/cert.conf" -extensions codesign \
    -keyout "$TMP/key.pem" -out "$TMP/cert.pem" >/dev/null 2>&1||err "Certificate generation failed."
  openssl pkcs12 -export -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -out "$TMP/key.p12" \
    -passout "pass:$password" >/dev/null 2>&1||err "Certificate conversion failed."
  security import "$TMP/key.p12" -k "$chain" -P "$password" \
    -T /usr/bin/codesign >/dev/null||err "Keychain import failed."
  # Restrict trust to user-level code signing, not SSL or the system trust store.
  security add-trusted-cert -r trustRoot -p codeSign -k "$chain" "$TMP/cert.pem" >/dev/null||
    err "User Code Signing trust failed. Inspect Keychain Access."
  h="$(hash)"
  [[ -n "$h" ]]||err "Signing identity is not valid."
  say "Signing key saved in login keychain. Never delete or regenerate it."
  printf '%s\n' "$h"
}
if [[ "${1:-}" == "--setup" ]];then setup >/dev/null;say "Local signing initialized.";exit 0;fi
if [[ "${1:-}" == "--check" ]];then
  h="$(hash)";[[ -n "$h" ]]||err "Run --setup first."
  say "Local signing identity: $h"
  if [[ -d "$DEST" ]];then codesign -d -r- "$DEST" 2>&1|grep 'designated =>'||true;fi
  exit 0
fi
SOURCE="${1:-$ROOT/kvm.app}"
[[ -e "$SOURCE" ]]||err "Supply the .dmg or .app path, or run from the mounted DMG."
work
case "$SOURCE" in
  *.dmg)
    mkdir -p "$TMP/mount"
    hdiutil attach "$SOURCE" -readonly -nobrowse -mountpoint "$TMP/mount" -quiet||
      err "Could not mount DMG."
    MOUNTED=1;SOURCE="$TMP/mount/kvm.app";;
  *.app) ;;
  *) err "Expected a .dmg or .app file.";;
esac
[[ -d "$SOURCE" && ! -L "$SOURCE" ]]||err "kvm.app is missing or a symlink."
[[ -f "$SOURCE/Contents/Info.plist" ]]||err "Invalid app."
bundle="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$SOURCE/Contents/Info.plist")"
[[ "$bundle" == "$ID" ]]||err "Unexpected bundle identifier: $bundle"
[[ -x "$SOURCE/Contents/MacOS/kvm" ]]||err "Missing kvm executable."
if pgrep -x kvm >/dev/null;then err "Quit running kvm from the menu bar first.";fi
ditto "$SOURCE" "$TMP/kvm.app"
h="$(setup|tail -n 1)"
[[ "$h" =~ ^[[:xdigit:]]{40}$ ]]||err "Invalid signing identity."
req="designated => identifier \"$ID\" and certificate leaf = H\"$h\""
codesign --force --sign "$h" --timestamp=none -r "$req" "$TMP/kvm.app"||
  err "Signing failed. Check your Keychain Access permissions."
codesign --verify --strict "$TMP/kvm.app"||err "Signed app could not be verified."
newdr="$(codesign -d -r- "$TMP/kvm.app" 2>&1|grep 'designated =>'|sed 's#^[[:space:]]*##')"
[[ -n "$newdr" ]]||err "Missing designated requirement."
if echo "$newdr"|grep -qi 'designated => cdhash';then err "Result is ad-hoc, not stable.";fi
if [[ -d "$DEST" ]];then
  olddr="$(codesign -d -r- "$DEST" 2>&1|grep 'designated =>'|sed 's#^[[:space:]]*##'||true)"
  if [[ "$olddr" != "$newdr" ]];then
    say "Switching from old signature; ONE last macOS Accessibility approval may be needed."
  fi
fi
NEXT="/Applications/.kvm-next.app";BACKUP="/Applications/.kvm-prev.app"
[[ ! -e "$NEXT" && ! -e "$BACKUP" ]]||err "Previous install incomplete (.kvm-next or .kvm-prev)."
say "Installing to $DEST (administrator password may be requested)."
sudo /usr/bin/ditto "$TMP/kvm.app" "$NEXT"
if ! sudo /usr/bin/codesign --verify --strict "$NEXT";then
  sudo /bin/rm -rf "$NEXT";err "Copied app failed verification; old app unchanged."
fi
if [[ -e "$DEST" ]];then sudo /bin/mv "$DEST" "$BACKUP";fi
if ! sudo /bin/mv "$NEXT" "$DEST";then
  if [[ -e "$BACKUP" ]];then sudo /bin/mv "$BACKUP" "$DEST";fi
  err "Install failed; previous version restored."
fi
if [[ -e "$BACKUP" ]];then sudo /bin/rm -rf "$BACKUP";fi
say "Installed. Open /Applications/kvm.app."
say "Reuse THIS updater for each release, not drag-and-drop."
say "No notarization: Gatekeeper may still request first-open approval."
