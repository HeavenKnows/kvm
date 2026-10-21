#!/bin/bash
# Regression tests for macOS security(1) keychain output formatting.
set -euo pipefail
cd "$(dirname "$0")/.."
TMP_TEST="$(mktemp -d)"
trap 'rm -rf "$TMP_TEST"' EXIT
mkdir -p "$TMP_TEST/bin" "$TMP_TEST/empty-home"
cat > "$TMP_TEST/bin/security" <<'MOCK'
#!/bin/bash
case "$1" in
  default-keychain) printf '    "%s"   \n' "${KVM_TEST_DEFAULT:-/not/a/keychain}";;
  list-keychains)   printf '  "%s"\n' "${KVM_TEST_LIST:-/not/a/keychain}";;
  show-keychain-info)
    [[ -f "$2" && "$2" != "${KVM_TEST_INVALID_KEYCHAIN:-}" ]] ;;
  find-identity)
    printf '  0 valid identities found\n' ;;
  *) exit 2;;
esac
MOCK
chmod +x "$TMP_TEST/bin/security"
KEYCHAIN="$TMP_TEST/测试 login keychain.keychain-db"
INVALID="$TMP_TEST/stale.keychain-db"
: > "$KEYCHAIN"
: > "$INVALID"
installer="scripts/macos-local-sign-install.sh"
run(){
  env HOME="$TMP_TEST/empty-home" PATH="$TMP_TEST/bin:$PATH" \
    /bin/bash "$installer" --check-keychain
}
# Default keychain: indentation, quotation marks, Unicode and spaces.
out="$(KVM_TEST_DEFAULT="$KEYCHAIN" run 2>&1)"
[[ "$out" == "kvm: Usable user keychain: $KEYCHAIN" ]]
# Invalid default: fall back to the valid search-list entry.
out="$(KVM_TEST_DEFAULT="$INVALID" KVM_TEST_INVALID_KEYCHAIN="$INVALID" KVM_TEST_LIST="$KEYCHAIN" run 2>&1)"
[[ "$out" == "kvm: Usable user keychain: $KEYCHAIN" ]]
# Explicit keychain selection succeeds and rejects a missing file instead
# of silently importing a signing key somewhere else.
out="$(KVM_KEYCHAIN_PATH="$KEYCHAIN" run 2>&1)"
[[ "$out" == "kvm: Usable user keychain: $KEYCHAIN" ]]
if KVM_KEYCHAIN_PATH="$TMP_TEST/does-not-exist" run >/dev/null 2>&1;then
  echo "Invalid explicit keychain must be rejected" >&2
  exit 1
fi
if KVM_TEST_DEFAULT="$TMP_TEST/missing" KVM_TEST_LIST="$TMP_TEST/absent" \
  run >/dev/null 2>&1;then
  echo "Missing user keychain must be rejected" >&2
  exit 1
fi
# A regression assertion for the old empty-array expansion: a valid keychain
# with no signing identity must still pass preflight without a nounset error.
out="$(KVM_TEST_DEFAULT="$KEYCHAIN" run 2>&1)"
[[ "$out" == "kvm: Usable user keychain: $KEYCHAIN" ]]
if printf '%s' "$out" | grep -q 'unbound variable';then
  echo "Empty keychain list must not abort under set -u" >&2
  exit 1
fi
echo "Mac keychain resolver regression tests passed."
