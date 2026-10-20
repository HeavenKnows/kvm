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
GUI=0
WAIT_PID=""
if [[ "${1:-}" == "--gui" ]];then
  GUI=1;shift
  if [[ "${1:-}" == "--wait-pid" ]];then
    WAIT_PID="${2:-}"
    [[ "$WAIT_PID" =~ ^[0-9]+$ ]]||{ echo "Invalid updater PID" >&2;exit 1; }
    shift 2
  fi
fi
show_dialog(){
  [[ "$GUI" == 1 ]]||return 0
  /usr/bin/osascript -e 'on run argv
    display dialog (item 1 of argv) buttons {"好"} default button 1 with title "kvm"
  end run' "$1" >/dev/null 2>&1||true
}
err(){ echo "kvm: $*" >&2;show_dialog "安装失败。请查看 ~/Library/Logs/kvm-updater.log。$*";exit 1; }
say(){ echo "kvm: $*"; }
clean(){
  if [[ "$MOUNTED" == 1 ]];then hdiutil detach "$TMP/mount" -quiet >/dev/null 2>&1||true;fi
  if [[ -n "$TMP" && -d "$TMP" ]];then rm -rf "$TMP";fi
  # The GUI updater uses a private disposable copy of this very script:
  # deleting it does not change the installed signed app.
  if [[ "${KVM_INSTALL_SCRIPT_COPY:-}" == 1 && "$GUI" == 1 ]];then
    rm -f -- "$0"
  fi
  # Remove downloaded DMG only after installation/unmount has completed.
  if [[ "$GUI" == 1 && -n "${KVM_AUTODOWNLOAD_DMG:-}" ]];then
    downloaded="${KVM_AUTODOWNLOAD_DMG%/}"
    download_temp_root="${TMPDIR:-/tmp}"
    if [[ "${downloaded##*/}" == kvm-update-*.dmg &&
          "${downloaded%/*}" == "${download_temp_root%/}" ]];then
      /bin/rm -f -- "$downloaded"
    fi
  fi
  # Clean only the temporary bundle snapshot prepared by macos_update.rs.
  # Never accept an arbitrary directory to delete.
  if [[ "$GUI" == 1 && -n "${KVM_STAGED_APP_DIRECTORY:-}" ]];then
    staged="${KVM_STAGED_APP_DIRECTORY%/}"
    temp_root="${TMPDIR:-/tmp}"
    if [[ "${staged##*/}" == kvm-source-* &&
          "${staged%/*}" == "${temp_root%/}" ]];then
      /bin/rm -rf -- "$staged"
    fi
  fi
}
trap clean EXIT
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]];then
  cat <<'USAGE'
kvm local signer and in-app updater (bundled within kvm.app)

First install: open the DMG and drag kvm.app to Applications.
Later: use kvm Overview -> Check for updates -> Download & install.
No shell script is needed for normal installation or updating.
Diagnostic only: bash install-kvm.sh --check-keychain

USAGE
  exit 0
fi
[[ "$(uname)" == Darwin ]]||err "Only supported on macOS."
[[ "$(id -u)" != 0 ]]||err "Run without sudo."
if [[ -n "$WAIT_PID" ]];then
  for ((i=0;i<300;i++));do
    kill -0 "$WAIT_PID" 2>/dev/null||break
    sleep 0.1
  done
  kill -0 "$WAIT_PID" 2>/dev/null&&err "Old kvm is still running; quit it first."
fi
# security output is display-formatted: quoted, sometimes indented.
# Normalize and validate the actual keychain before importing a private key.
normalize_keychain(){
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  if [[ "$value" == \"*\" ]];then value="${value:1:${#value}-2}";fi
  if [[ "$value" == "~/"* ]];then value="$HOME/${value:2}";fi
  [[ "$value" == /* && -f "$value" ]]||return 1
  printf '%s\n' "$value"
}
resolve_keychain(){
  local raw candidate
  if [[ -n "${KVM_KEYCHAIN_PATH:-}" ]];then
    normalize_keychain "$KVM_KEYCHAIN_PATH" ||
      { echo "kvm: KVM_KEYCHAIN_PATH does not point to an existing keychain." >&2;return 1; }
    return
  fi
  raw="$(security default-keychain -d user 2>/dev/null || true)"
  if candidate="$(normalize_keychain "$raw")";then printf '%s\n' "$candidate";return;fi
  while IFS= read -r raw;do
    if candidate="$(normalize_keychain "$raw")";then
      printf '%s\n' "$candidate";return
    fi
  done < <(security list-keychains -d user 2>/dev/null || true)
  for raw in "$HOME/Library/Keychains/login.keychain-db" "$HOME/Library/Keychains/login.keychain";do
    if candidate="$(normalize_keychain "$raw")";then
      printf '%s\n' "$candidate";return
    fi
  done
  echo "kvm: No accessible user keychain. Open Keychain Access, restore/select your login keychain, then retry." >&2
  return 1
}
hash(){
  security find-identity -v -p codesigning "$1" 2>/dev/null | awk -F'"' -v cn="$NAME" \
    '$2==cn{n=$1;sub(/^[[:space:]]+[0-9]+\)[[:space:]]+/,"",n);gsub(/[[:space:]]/,"",n);print n;exit}'
}
work(){ if [[ -z "$TMP" ]];then TMP="$(mktemp -d "${TMPDIR:-/tmp/}kvm-sign.XXXXXXXX")";fi; }
setup(){
  local h chain password
  chain="$(resolve_keychain)" || err "未找到可用的用户钥匙串。请在“钥匙串访问”中检查登录钥匙串。"
  h="$(hash "$chain")"
  if [[ -n "$h" ]];then printf '%s\n' "$h";return;fi
  if security find-certificate -c "$NAME" "$chain" >/dev/null 2>&1;then
    err "Existing local certificate is unavailable for code signing in the selected keychain; unlock or repair it in Keychain Access."
  fi
  work
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
  h="$(hash "$chain")"
  [[ -n "$h" ]]||err "Signing identity is not valid in the selected user keychain."
  say "Signing key saved in login keychain. Never delete or regenerate it."
  printf '%s\n' "$h"
}
if [[ "${1:-}" == "--check-keychain" ]];then
  chain="$(resolve_keychain)"||err "没有可用的用户登录钥匙串。"
  say "Usable user keychain: $chain"
  exit 0
fi
if [[ "${1:-}" == "--setup" ]];then setup >/dev/null;say "Local signing initialized.";exit 0;fi
if [[ "${1:-}" == "--check" ]];then
  chain="$(resolve_keychain)"||err "没有可用的用户登录钥匙串。"
  h="$(hash "$chain")";[[ -n "$h" ]]||err "Run --setup first."
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
# First verify the downloaded app bundle's original signature. This detects
# corruption before we replace its signature with the user's stable identity.
codesign --verify --strict "$SOURCE" ||
  err "Downloaded kvm.app is modified or corrupt. Download it again from the project Actions."
if pgrep -x kvm >/dev/null;then err "Quit running kvm from the menu bar first.";fi
ditto "$SOURCE" "$TMP/kvm.app"
h="$(setup|tail -n 1)"||err "Could not initialize the personal signing certificate."
[[ "$h" =~ ^[[:xdigit:]]{40}$ ]]||err "Invalid signing identity."
chain="$(resolve_keychain)"||err "签名钥匙串不可用。"
req="designated => identifier \"$ID\" and certificate leaf = H\"$h\""
codesign --force --sign "$h" --keychain "$chain" --timestamp=none -r "$req" "$TMP/kvm.app"||
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
if [[ "$GUI" == 1 ]];then
  # Build ONE privileged, transactional copy/rename command. Private
  # code-signing keys are never used with root privileges.
  shell_quote(){
    local escaped="${1//\'/\'\\\'\'}"
    printf "'%s'" "$escaped"
  }
  source_q="$(shell_quote "$TMP/kvm.app")"
  admin_command="set -e; /usr/bin/ditto $source_q /Applications/.kvm-next.app; "
  admin_command+=" /usr/bin/codesign --verify --strict /Applications/.kvm-next.app; "
  admin_command+=" if test -e /Applications/kvm.app; then /bin/mv /Applications/kvm.app /Applications/.kvm-prev.app; fi; "
  admin_command+=" if ! /bin/mv /Applications/.kvm-next.app /Applications/kvm.app; then "
  admin_command+=" if test -e /Applications/.kvm-prev.app; then /bin/mv /Applications/.kvm-prev.app /Applications/kvm.app; fi; exit 1; fi; "
  admin_command+=" if test -e /Applications/.kvm-prev.app; then /bin/rm -rf /Applications/.kvm-prev.app; fi"
  # Quote for AppleScript's STRING literal, not for AppleScript code.
  applescript_string="${admin_command//\\/\\\\}"
  applescript_string="${applescript_string//\"/\\\"}"
  /usr/bin/osascript -e "do shell script \"$applescript_string\" with administrator privileges" ||
    err "Administrator authorization or installation failed."
else
  sudo /usr/bin/ditto "$TMP/kvm.app" "$NEXT"
  if ! /usr/bin/codesign --verify --strict "$NEXT";then
    sudo /bin/rm -rf "$NEXT";err "Copied app failed verification; old app unchanged."
  fi
  if [[ -e "$DEST" ]];then sudo /bin/mv "$DEST" "$BACKUP";fi
  if ! sudo /bin/mv "$NEXT" "$DEST";then
    if [[ -e "$BACKUP" ]];then sudo /bin/mv "$BACKUP" "$DEST";fi
    err "Install failed; previous version restored."
  fi
  if [[ -e "$BACKUP" ]];then sudo /bin/rm -rf "$BACKUP";fi
fi
say "Installed. Open /Applications/kvm.app."
if [[ "$GUI" == 1 ]];then
  /usr/bin/open "/Applications/kvm.app" ||
    show_dialog "安装已完成，请从“应用程序”打开 kvm。"
fi
say "Reuse THIS updater for each release, not drag-and-drop."
say "No Apple notarization: first-open approval may still be required."
say "If macOS explicitly reports malware, do not override that security warning."
