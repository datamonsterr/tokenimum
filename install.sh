#!/usr/bin/env bash
# Download and verify the three executables produced by .github/workflows/release.yml.
set -euo pipefail

main() {
  local version=latest method=auto archive_path= bin_dir="${HOME:?HOME is not set}/.local/bin"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --version|--method|--bin-dir|--archive)
        [ "$#" -ge 2 ] && [ -n "$2" ] || die "$1 needs a value"
        case "$1" in
          --version) version=$2 ;;
          --method) method=$2 ;;
          --bin-dir) bin_dir=$2 ;;
          --archive) archive_path=$2 ;;
        esac
        shift 2 ;;
      -h|--help)
        cat <<EOF
Install or update tokenimum's Linux release binaries (no root required).

Usage: bash install.sh [options]
  --version TAG    Release tag, e.g. v0.0.0 (default: latest stable release)
  --method METHOD auto, gh, or curl (default: prefer gh when available)
  --bin-dir DIR   Absolute installation directory (current: $bin_dir)
  --archive FILE  Install a local release .tar.gz with its adjacent .sha256 file
  --help          Show this help

Supports Linux x86_64 and ARM64, including Linux inside WSL2.
Private repositories: authenticate gh, or use curl with GH_TOKEN and jq.
No stable release yet? Select a preview explicitly with --version v0.0.0.
Re-run with the same --bin-dir to update. Restart tkmd and your coding agent afterward.
EOF
        return ;;
      *) die "Unknown option: $1 (see --help)" ;;
    esac
  done

  case "$method" in auto|gh|curl) ;; *) die "Invalid --method: $method" ;; esac
  [[ "$version" == latest || "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-.][A-Za-z0-9.-]+)?$ ]] ||
    die "Invalid version: use a release tag such as v0.0.0, or latest"
  [[ "$bin_dir" == /* ]] || die "--bin-dir must be an absolute path"

  local system architecture target archive
  system=$(uname -s)
  [ "$system" = Linux ] || die "Unsupported OS: $system. Release binaries support Linux only; on Windows use WSL2."
  architecture=$(uname -m)
  case "$architecture" in
    x86_64|amd64) target=x86_64-unknown-linux-musl ;;
    aarch64|arm64) target=aarch64-unknown-linux-musl ;;
    *) die "Unsupported architecture: $architecture. Releases support x86_64 and ARM64 only." ;;
  esac
  archive="tokenimum-$target.tar.gz"
  local utility
  for utility in tar sha256sum install mktemp cp mv; do
    command -v "$utility" >/dev/null || die "Required command missing: $utility"
  done

  work=$(mktemp -d)
  stage=
  trap 'rm -rf -- "$work"; if [ -n "$stage" ]; then rm -rf -- "$stage"; fi' EXIT

  if [ -n "$archive_path" ]; then
    [ "${archive_path##*/}" = "$archive" ] || die "Expected local archive named $archive for this machine"
    cp -- "$archive_path" "$work/$archive"
    cp -- "$archive_path.sha256" "$work/$archive.sha256"
  else
    if [ "$method" = auto ]; then
      if command -v gh >/dev/null; then method=gh; else method=curl; fi
    fi
    printf 'Downloading tokenimum %s for %s using %s...\n' "$version" "$target" "$method"
    case "$method" in
      gh)
        command -v gh >/dev/null || die "Install GitHub CLI and run gh auth login, or use --method curl"
        local release_args=()
        if [ "$version" != latest ]; then release_args=("$version"); fi
        gh release download "${release_args[@]}" --repo datamonsterr/tokenimum \
          --pattern "$archive" --pattern "$archive.sha256" --dir "$work" ||
          die "Download failed. Check repository access (gh auth login) and the release tag. For the preview use --version v0.0.0."
        ;;
      curl)
        command -v curl >/dev/null || die "Required command missing: curl"
        local token="${GH_TOKEN:-${GITHUB_TOKEN:-}}" base_url asset asset_url
        if [ -n "$token" ]; then
          command -v jq >/dev/null || die "Authenticated curl downloads require jq; alternatively use --method gh"
          if [ "$version" = latest ]; then
            base_url=https://api.github.com/repos/datamonsterr/tokenimum/releases/latest
          else
            base_url="https://api.github.com/repos/datamonsterr/tokenimum/releases/tags/$version"
          fi
          fetch "$base_url" "$work/release.json" application/vnd.github+json
          for asset in "$archive" "$archive.sha256"; do
            asset_url=$(jq -er --arg name "$asset" '.assets[] | select(.name == $name) | .url' "$work/release.json") ||
              die "Release asset missing: $asset"
            [[ "$asset_url" == https://api.github.com/repos/datamonsterr/tokenimum/releases/assets/* ]] ||
              die "Unexpected release asset URL"
            fetch "$asset_url" "$work/$asset" application/octet-stream
          done
        else
          if [ "$version" = latest ]; then
            base_url=https://github.com/datamonsterr/tokenimum/releases/latest/download
          else
            base_url="https://github.com/datamonsterr/tokenimum/releases/download/$version"
          fi
          for asset in "$archive" "$archive.sha256"; do
            fetch "$base_url/$asset" "$work/$asset" application/octet-stream
          done
        fi
        ;;
    esac
  fi

  (
    cd "$work"
    [ "$(sha256sum "$archive")" = "$(cat "$archive.sha256")" ] || die "Archive checksum mismatch"
    sha256sum --check "$archive.sha256"
    tar -xzf "$archive" --no-same-owner tkm tkm-hook tkmd SHA256SUMS
    for binary in tkm tkm-hook tkmd SHA256SUMS; do
      [ -f "$binary" ] && [ ! -L "$binary" ] || die "Invalid release file: $binary"
    done
    sha256sum --check SHA256SUMS
    [ "$(sha256sum tkm tkm-hook tkmd)" = "$(cat SHA256SUMS)" ] || die "Incomplete binary checksums"
  )

  mkdir -p -- "$bin_dir"
  local binary
  for binary in tkm tkm-hook tkmd; do
    [ ! -d "$bin_dir/$binary" ] || die "Installation destination is a directory: $bin_dir/$binary"
  done
  # Stage beside the destination so rename works even while an old executable is running.
  stage=$(mktemp -d "$bin_dir/.tokenimum.XXXXXX")
  install -m755 "$work/tkm" "$work/tkm-hook" "$work/tkmd" "$stage/"
  for binary in tkm tkm-hook tkmd; do
    mv -fT -- "$stage/$binary" "$bin_dir/$binary"
  done
  printf '\nInstalled tkm, tkm-hook, and tkmd in %s\n' "$bin_dir"
  printf 'Add this directory to PATH in your shell startup file, then open a new terminal:\n'
  printf '  export PATH="%s:$PATH"\n' "$bin_dir"
  printf 'First install: run tkm init --yes --preset offline inside your project.\n'
  printf 'Updating: restart tkmd and your coding agent, then run tkm doctor.\n'
}

die() { printf 'tokenimum installer: %s\n' "$*" >&2; exit 1; }

fetch() {
  local headers=(-H "Accept: $3")
  if [ -n "$token" ]; then headers+=(-H "Authorization: Bearer $token"); fi
  curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
    "${headers[@]}" "$1" --output "$2" ||
    die "Download failed. Private repositories need GH_TOKEN with repository read access (and jq), or --method gh. Check the tag; previews require --version v0.0.0."
}

main "$@"
