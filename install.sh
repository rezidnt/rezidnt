#!/bin/sh
# rezidnt installer — DR-037 sub-slice `install-script`. Clean-room (I8), written
# from scratch. The golden path's `curl | sh` step (§1/§18): fetch the two static
# musl binaries published by the `release-ci` workflow, VERIFY each against the
# published sha256 BEFORE installing (fail-closed on mismatch — I6, no half-install),
# and place `rezidnt` + `rezidentd` on PATH. Linux/WSL-first (DR-037 scope fence): a
# non-Linux OS or non-x86_64 arch is refused in plain language, installing nothing.
# No telemetry, no phone-home (I7): the only network egress is fetching the release
# assets you asked for.
#
# Override seams (env), all optional:
#   REZIDNT_INSTALL_DIR  where the binaries go            (default: $HOME/.local/bin)
#   REZIDNT_VERSION      release tag to install           (default: latest release)
#   REZIDNT_REPO         GitHub owner/repo                 (default: rezidnt/rezidnt)
#   REZIDNT_BASE_URL     asset base URL; supports file://  (default: the release URL)
#   REZIDNT_OS/ARCH      override uname -s / uname -m      (default: detected)
#
# Exit codes: 0 ok · 1 usage · 2 unsupported platform · 3 fetch/resolve · 4 checksum.

set -eu

REPO="${REZIDNT_REPO:-rezidnt/rezidnt}"
INSTALL_DIR="${REZIDNT_INSTALL_DIR:-$HOME/.local/bin}"
OS="${REZIDNT_OS:-$(uname -s)}"
ARCH="${REZIDNT_ARCH:-$(uname -m)}"

say() { printf 'rezidnt: %s\n' "$1"; }
err() { printf 'rezidnt: %s\n' "$1" >&2; }

# --- Platform gate (DR-037 Linux/WSL-first scope fence; I6) --------------------
# WSL reports OS=Linux, so a single Linux check covers the supported hosts.
case "$OS" in
  Linux) ;;
  *)
    err "install.sh supports Linux/WSL only (DR-037 is Linux/WSL-first); detected OS '$OS'."
    err "macOS and native Windows are not yet supported — their substrates are not built."
    exit 2
    ;;
esac
case "$ARCH" in
  x86_64 | amd64) TARGET="x86_64-unknown-linux-musl" ;;
  *)
    err "install.sh supports x86_64 only right now (aarch64 is deferred in DR-037); \
detected arch '$ARCH'."
    exit 2
    ;;
esac

# --- Tools: without sha256sum we cannot verify, so we refuse (never install
# unverified bytes — I6) --------------------------------------------------------
if ! command -v sha256sum >/dev/null 2>&1; then
  err "sha256sum not found — cannot verify the download; refusing to install unverified bytes."
  exit 4
fi

# --- Resolve version + base URL ------------------------------------------------
VERSION="${REZIDNT_VERSION:-}"
BASE_URL="${REZIDNT_BASE_URL:-}"
if [ -z "$BASE_URL" ]; then
  if [ -z "$VERSION" ]; then
    # Newest-release resolution (real use only; not exercised by the file:// fixture
    # tests). Use /releases (newest first) NOT /releases/latest — the latter EXCLUDES
    # pre-releases, and pre-1.0 rezidnt ships only pre-releases. Parse the first
    # tag_name without a jq dependency.
    command -v curl >/dev/null 2>&1 || { err "curl not found — needed to resolve the latest release."; exit 3; }
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=1" \
      | grep '"tag_name"' | head -1 | sed -e 's/.*"tag_name":[ ]*"//' -e 's/".*//')
    [ -n "$VERSION" ] || { err "could not resolve the newest release tag for $REPO (no releases yet?)."; exit 3; }
  fi
  BASE_URL="https://github.com/$REPO/releases/download/$VERSION"
fi
BASE_URL="${BASE_URL%/}" # tolerate a trailing slash

# --- Fetch: file:// via cp (mirror / air-gapped / tests), else curl ------------
fetch() { # <url> <dest>
  case "$1" in
    file://*) cp "${1#file://}" "$2" ;;
    *)
      command -v curl >/dev/null 2>&1 || { err "curl not found — needed to download $1"; exit 3; }
      curl -fsSL "$1" -o "$2"
      ;;
  esac
}

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM

REZIDNT_ASSET="rezidnt-$TARGET"
REZIDENTD_ASSET="rezidentd-$TARGET"

say "installing $REPO ${VERSION:+$VERSION }($TARGET) into $INSTALL_DIR"
for asset in "$REZIDNT_ASSET" "$REZIDENTD_ASSET" SHA256SUMS; do
  if ! fetch "$BASE_URL/$asset" "$WORK/$asset"; then
    err "failed to download $asset from $BASE_URL"
    exit 3
  fi
done

# --- Verify EVERYTHING before installing ANYTHING (fail-closed; I6) ------------
# SHA256SUMS lists both binaries; `sha256sum -c` fails if either mismatches or is
# missing, so a single failure aborts before any file is placed (all-or-nothing).
if ! (cd "$WORK" && sha256sum -c SHA256SUMS >/dev/null 2>&1); then
  err "checksum verification FAILED — the download does not match the published SHA256SUMS."
  err "nothing was installed (fail-closed)."
  exit 4
fi

# --- Install (only now that both are verified) --------------------------------
mkdir -p "$INSTALL_DIR"
install_bin() { # <asset> <name>
  cp "$WORK/$1" "$INSTALL_DIR/$2"
  chmod +x "$INSTALL_DIR/$2"
}
install_bin "$REZIDNT_ASSET" rezidnt
install_bin "$REZIDENTD_ASSET" rezidentd

say "installed rezidnt + rezidentd to $INSTALL_DIR"

# --- PATH guidance (non-fatal) -------------------------------------------------
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    say "note: $INSTALL_DIR is not on your PATH."
    say "add it, e.g.:  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

say "next: start the daemon (rezidentd) in its own terminal, then run 'rezidnt init'."
