#!/bin/sh

set -eu

RELEASE="${CODEX_RELEASE:-latest}"
NON_INTERACTIVE="${CODEX_NON_INTERACTIVE:-false}"
DAEMON_ONLY="${CODEX_INSTALL_DAEMON_ONLY:-0}"

BIN_DIR="${CODEX_INSTALL_DIR:-$HOME/.local/bin}"
BIN_PATH="$BIN_DIR/codex"
CODE_MODE_HOST_BIN_PATH="$BIN_DIR/codex-code-mode-host"
CODEX_HOME_DIR="${CODEX_HOME:-$HOME/.codex}"
STANDALONE_ROOT="$CODEX_HOME_DIR/packages/asm-standalone"
RELEASES_DIR="$STANDALONE_ROOT/releases"
CURRENT_LINK="$STANDALONE_ROOT/current"
LOCK_FILE="$STANDALONE_ROOT/install.lock"
LOCK_DIR="$STANDALONE_ROOT/install.lock.d"
LOCK_STALE_AFTER_SECS=600

path_action="already"
path_profile=""
conflict_manager=""
lock_kind=""
tmp_dir=""

step() {
  printf '==> %s\n' "$1"
}

warn() {
  printf 'WARNING: %s\n' "$1" >&2
}

normalize_version() {
  case "$1" in
    "" | latest)
      printf 'latest\n'
      ;;
    v*)
      printf '%s\n' "${1#v}"
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}

validate_version() {
  version="$1"

  if [ "$version" = "latest" ]; then
    return
  fi

  if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-(alpha|beta)(\.[0-9]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'; then
    echo "Invalid Codex release version: $version. Expected latest or x.y.z[-alpha|-beta], with optional .N numeric parts and +build metadata." >&2
    return 1
  fi
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --release)
        if [ "$#" -lt 2 ]; then
          echo "--release requires a value." >&2
          exit 1
        fi
        RELEASE="$2"
        shift
        ;;
      --help | -h)
        cat <<EOF
Usage: install.sh [--release VERSION]

Environment:
  CODEX_RELEASE          Version to install; overridden by --release.
  CODEX_NON_INTERACTIVE  Set to 1, true, or yes to skip prompts.
  CODEX_HOME             Root for ASM's standalone package state.
  CODEX_INSTALL_DIR      Directory for the codex and helper command links.
EOF
        exit 0
        ;;
      *)
        echo "Unknown argument: $1" >&2
        exit 1
        ;;
    esac
    shift
  done
}

download_file() {
  url="$1"
  output="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$output" "$url"
  else
    echo "curl or wget is required to install ASM." >&2
    exit 1
  fi
}

download_text() {
  url="$1"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O - "$url"
  else
    echo "curl or wget is required to install ASM." >&2
    exit 1
  fi
}

parse_release_metadata() {
  # Bound awk's record size so compact, single-line JSON stays fast on every
  # supported awk implementation. JSON strings cannot contain literal newlines,
  # so the record boundaries inserted by fold do not change the document.
  LC_ALL=C fold -b -w 4096 | LC_ALL=C awk '
    function finish_string(value) {
      if (object_depth == 1 && key == "tag_name") {
        print "tag_name\t" value
      } else if (object_depth == asset_object_depth) {
        if (key == "name") {
          asset_name = value
        } else if (key == "digest") {
          asset_digest = value
        }
      }

      expecting_value = 0
      key = ""
    }

    {
      for (i = 1; i <= length($0); i++) {
        char = substr($0, i, 1)

        if (in_string) {
          if (escaped) {
            token = token "\\" char
            escaped = 0
          } else if (char == "\\") {
            escaped = 1
          } else if (char == "\"") {
            in_string = 0
            if (string_is_value) {
              finish_string(token)
            } else {
              pending_key = token
            }
          } else {
            token = token char
          }
          continue
        }

        if (char == "\"") {
          in_string = 1
          token = ""
          escaped = 0
          string_is_value = expecting_value
        } else if (char == ":" && pending_key != "") {
          key = pending_key
          pending_key = ""
          expecting_value = 1
        } else if (char == "{") {
          object_depth++
          if (assets_array_depth != 0 &&
              array_depth == assets_array_depth &&
              asset_object_depth == 0) {
            asset_object_depth = object_depth
            asset_name = ""
            asset_digest = ""
          }
          expecting_value = 0
          key = ""
        } else if (char == "}") {
          if (object_depth == asset_object_depth) {
            if (asset_name != "") {
              print "asset\t" asset_name "\t" asset_digest
            }
            asset_object_depth = 0
            asset_name = ""
            asset_digest = ""
          }
          object_depth--
          expecting_value = 0
          key = ""
          pending_key = ""
        } else if (char == "[") {
          array_depth++
          if (expecting_value && key == "assets" && object_depth == 1) {
            assets_array_depth = array_depth
          }
          expecting_value = 0
          key = ""
        } else if (char == "]") {
          if (array_depth == assets_array_depth) {
            assets_array_depth = 0
          }
          array_depth--
          expecting_value = 0
          key = ""
          pending_key = ""
        } else if (char == ",") {
          expecting_value = 0
          key = ""
          pending_key = ""
        }
      }
    }

    END {
      if (in_string || object_depth != 0 || array_depth != 0) {
        exit 1
      }
    }
  '
}

release_url_for_asset() {
  printf 'https://github.com/trungnt13/asm/releases/download/v%s/%s\n' "$2" "$1"
}

release_metadata_url() {
  printf 'https://api.github.com/repos/trungnt13/asm/releases/tags/v%s\n' "$1"
}

parse_downloaded_release_metadata() {
  requested_release="$1"
  source_name="$2"
  if ! release_metadata="$(printf '%s\n' "$release_json" | parse_release_metadata)"; then
    echo "Could not parse $source_name release metadata for Codex $requested_release." >&2
    return 1
  fi
}

resolve_metadata_version() {
  release_tag="$(printf '%s\n' "$release_metadata" | awk -F '\t' '$1 == "tag_name" { print $2; exit }')"
  case "$release_tag" in
    v*) metadata_version="${release_tag#v}" ;;
    *) metadata_version="" ;;
  esac
  if [ -z "$metadata_version" ]; then
    echo "Failed to resolve the latest Codex release version." >&2
    return 1
  fi
  validate_version "$metadata_version"
}

resolve_release_from_github() {
  normalized_version="$1"
  if [ "$normalized_version" = "latest" ]; then
    requested_release="latest"
    metadata_url="https://api.github.com/repos/trungnt13/asm/releases/latest"
  else
    resolved_version="$normalized_version"
    requested_release="$resolved_version"
    metadata_url="$(release_metadata_url "$resolved_version")"
  fi

  if ! release_json="$(download_text "$metadata_url")"; then
    echo "Could not fetch GitHub release metadata for Codex $requested_release. GitHub API may be unavailable or rate limited." >&2
    exit 1
  fi

  parse_downloaded_release_metadata "$requested_release" "GitHub"

  if [ "$normalized_version" = "latest" ]; then
    resolve_metadata_version
    resolved_version="$metadata_version"
  fi

}

resolve_release() {
  normalized_version="$(normalize_version "$RELEASE")"
  validate_version "$normalized_version"
  resolve_release_from_github "$normalized_version"
  select_release_assets
}

release_asset_digest_or_empty() {
  asset="$1"

  digest="$(printf '%s\n' "$release_metadata" | awk -F '\t' -v asset="$asset" '
    $1 == "asset" && $2 == asset {
      print $3
      exit
    }
  ')"

  case "$digest" in
    sha256:????????????????????????????????????????????????????????????????)
      digest="${digest#sha256:}"
      case "$digest" in
        *[!0-9a-fA-F]*) return 1 ;;
      esac
      printf '%s\n' "$digest"
      ;;
    *)
      return 1
      ;;
  esac
}

release_asset_exists() {
  asset="$1"

  printf '%s\n' "$release_metadata" | awk -F '\t' -v asset="$asset" '
    $1 == "asset" && $2 == asset { found = 1 }
    END { exit !found }
  '
}

release_asset_digest() {
  asset="$1"

  digest="$(release_asset_digest_or_empty "$asset" || true)"
  if [ -z "$digest" ]; then
    echo "Could not find SHA-256 digest for release asset $asset." >&2
    exit 1
  fi

  printf '%s\n' "$digest"
}

select_release_assets() {
  package_asset="codex-$vendor_target.tar.gz"
  checksum_asset="SHA256SUMS-$vendor_target"
  if ! release_asset_exists "$checksum_asset"; then
    checksum_asset="SHA256SUMS"
  fi
  if [ "$vendor_target" = "x86_64-unknown-linux-gnu" ] &&
    ! release_asset_exists "$package_asset" &&
    release_asset_exists "codex-x86_64-unknown-linux-musl.tar.gz"; then
    echo "ASM release $resolved_version is MUSL-only; this installer requires $package_asset. Use that release's original installer for older releases." >&2
    return 1
  fi
  if ! release_asset_exists "$package_asset" || ! release_asset_exists "$checksum_asset"; then
    echo "Missing ASM release archive ($package_asset) or $checksum_asset for $resolved_version." >&2
    return 1
  fi
  asset="$package_asset"
  download_url="$(release_url_for_asset "$asset" "$resolved_version")"
  checksum_url="$(release_url_for_asset "$checksum_asset" "$resolved_version")"
}

package_archive_digest() {
  asset="$1"
  manifest_path="$2"

  digest="$(awk -v asset="$asset" '
    $2 == asset && length($1) == 64 && $1 !~ /[^0-9a-fA-F]/ {
      print tolower($1)
      found = 1
      exit
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' "$manifest_path" 2>/dev/null || true)"

  if [ -z "$digest" ]; then
    echo "Could not find SHA-256 digest for $asset in SHA256SUMS." >&2
    return 1
  fi

  printf '%s\n' "$digest"
}

file_sha256() {
  path="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
    return
  fi

  if command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$path" | sed 's/^.*= //'
    return
  fi

  echo "sha256sum, shasum, or openssl is required to verify the Codex download." >&2
  exit 1
}

verify_archive_digest() {
  verify_path="$1"
  verify_expected_digest="$2"
  verify_actual_digest="$(file_sha256 "$verify_path")"

  if [ "$verify_actual_digest" != "$verify_expected_digest" ]; then
    echo "Downloaded ASM asset checksum did not match expected digest." >&2
    echo "expected: $verify_expected_digest" >&2
    echo "actual:   $verify_actual_digest" >&2
    return 1
  fi
}

require_linux_glibc() {
  libc_version="$(getconf GNU_LIBC_VERSION 2>/dev/null || true)"
  if ! printf '%s\n' "$libc_version" | awk '
    /^glibc [0-9]+\.[0-9]+$/ {
      split($2, version, ".")
      supported = version[1] > 2 || (version[1] == 2 && version[2] >= 35)
    }
    END { exit !supported }
  '; then
    echo "ASM Linux releases require glibc 2.35 or newer (Ubuntu 22.04+); detected ${libc_version:-unknown or unsupported libc}." >&2
    exit 1
  fi
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required to install Codex." >&2
    exit 1
  fi
}

pick_profile() {
  # Use the same shell-specific split Homebrew documents because there is no
  # universal startup file across macOS/Linux login and interactive shells.
  case "$os:${SHELL:-}" in
    darwin:*/zsh)
      printf '%s\n' "$HOME/.zprofile"
      ;;
    darwin:*/bash)
      printf '%s\n' "$HOME/.bash_profile"
      ;;
    linux:*/zsh)
      printf '%s\n' "$HOME/.zshrc"
      ;;
    linux:*/bash)
      printf '%s\n' "$HOME/.bashrc"
      ;;
    *)
      printf '%s\n' "$HOME/.profile"
      ;;
  esac
}

add_to_path() {
  path_action="already"
  path_profile=""

  case ":$PATH:" in
    *":$BIN_DIR:"*)
      if [ -z "$conflict_manager" ]; then
        return
      fi
      ;;
  esac

  profile="$(pick_profile)"
  path_profile="$profile"
  begin_marker="# >>> Codex installer >>>"
  end_marker="# <<< Codex installer <<<"
  path_line="export PATH=\"$BIN_DIR:\$PATH\""

  if [ -f "$profile" ] && grep -F "$begin_marker" "$profile" >/dev/null 2>&1; then
    if grep -F "$path_line" "$profile" >/dev/null 2>&1; then
      path_action="configured"
      return
    fi

    if grep -F "$end_marker" "$profile" >/dev/null 2>&1; then
      rewrite_path_block "$profile" "$begin_marker" "$end_marker" "$path_line"
      path_action="updated"
      return
    fi
  fi

  append_path_block "$profile" "$begin_marker" "$end_marker" "$path_line"
  path_action="added"
}

append_path_block() {
  profile="$1"
  begin_marker="$2"
  end_marker="$3"
  path_line="$4"

  {
    printf '\n%s\n' "$begin_marker"
    printf '%s\n' "$path_line"
    printf '%s\n' "$end_marker"
  } >>"$profile"
}

rewrite_path_block() {
  profile="$1"
  begin_marker="$2"
  end_marker="$3"
  path_line="$4"
  tmp_profile="$tmp_dir/profile.$$.tmp"

  awk -v begin="$begin_marker" -v end="$end_marker" -v line="$path_line" '
    BEGIN {
      in_block = 0
      replaced = 0
    }
    $0 == begin {
      if (!replaced) {
        print begin
        print line
        print end
        replaced = 1
      }
      in_block = 1
      next
    }
    in_block {
      if ($0 == end) {
        in_block = 0
      }
      next
    }
    {
      print
    }
    END {
      if (in_block != 0) {
        exit 1
      }
    }
  ' "$profile" >"$tmp_profile"
  mv "$tmp_profile" "$profile"
}

mkdir_lock_is_stale() {
  [ -d "$LOCK_DIR" ] || return 1

  pid="$(cat "$LOCK_DIR/pid" 2>/dev/null || true)"
  started_at="$(cat "$LOCK_DIR/started_at" 2>/dev/null || true)"
  now="$(date +%s 2>/dev/null || printf '0')"

  case "$started_at" in
    ''|*[!0-9]*)
      started_at=0
      ;;
  esac

  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    return 1
  fi

  if [ "$started_at" -eq 0 ] || [ "$now" -eq 0 ]; then
    return 0
  fi

  [ $((now - started_at)) -ge "$LOCK_STALE_AFTER_SECS" ]
}

acquire_install_lock() {
  mkdir -p "$STANDALONE_ROOT"

  if [ "$os" = "darwin" ] && command -v lockf >/dev/null 2>&1; then
    : >>"$LOCK_FILE"
    exec 9<>"$LOCK_FILE"
    lockf 9
    lock_kind="lockf"
    return
  fi

  if command -v flock >/dev/null 2>&1; then
    exec 9>"$LOCK_FILE"
    flock 9
    lock_kind="flock"
    return
  fi

  while ! mkdir "$LOCK_DIR" 2>/dev/null; do
    if mkdir_lock_is_stale; then
      warn "Removing stale installer lock at $LOCK_DIR"
      rm -rf "$LOCK_DIR"
      continue
    fi
    sleep 1
  done

  printf '%s\n' "$$" >"$LOCK_DIR/pid"
  date +%s >"$LOCK_DIR/started_at" 2>/dev/null || true
  lock_kind="mkdir"
}

release_install_lock() {
  if [ "$lock_kind" = "mkdir" ]; then
    rm -rf "$LOCK_DIR" 2>/dev/null || true
  elif [ "$lock_kind" = "flock" ] || [ "$lock_kind" = "lockf" ]; then
    exec 9>&- 2>/dev/null || true
  fi
  lock_kind=""
}

cleanup_stale_install_artifacts() {
  mkdir -p "$RELEASES_DIR" "$STANDALONE_ROOT"

  find "$RELEASES_DIR" -mindepth 1 -maxdepth 1 -name '.staging.*' -exec rm -rf {} +
  find "$STANDALONE_ROOT" -mindepth 1 -maxdepth 1 -name '.current.*' -exec rm -f {} +

  if [ -d "$BIN_DIR" ]; then
    find "$BIN_DIR" -mindepth 1 -maxdepth 1 -name '.codex.*' -exec rm -f {} +
  fi
}

replace_path_with_symlink() {
  link_path="$1"
  link_target="$2"
  tmp_link="$3"

  rm -f "$tmp_link"
  ln -s "$link_target" "$tmp_link"

  if mv -Tf "$tmp_link" "$link_path" 2>/dev/null; then
    return
  fi

  if mv -hf "$tmp_link" "$link_path" 2>/dev/null; then
    return
  fi

  rm -f "$link_path"
  mv -f "$tmp_link" "$link_path"
}

version_from_binary() {
  codex_path="$1"

  if [ ! -x "$codex_path" ]; then
    return 1
  fi

  "$codex_path" --version 2>/dev/null | sed -n 's/.* \([0-9][0-9A-Za-z.+-]*\)$/\1/p' | head -n 1
}

current_installed_version() {
  version="$(version_from_binary "$CURRENT_LINK/bin/codex" || true)"
  if [ -n "$version" ]; then
    printf '%s\n' "$version"
    return 0
  fi

  version="$(version_from_binary "$CURRENT_LINK/codex" || true)"
  if [ -n "$version" ]; then
    printf '%s\n' "$version"
    return 0
  fi

  return 0
}

resolve_existing_codex() {
  command -v codex 2>/dev/null || true
}

classify_existing_codex() {
  existing_path="$1"

  if [ -z "$existing_path" ] || [ "$existing_path" = "$BIN_PATH" ]; then
    return 1
  fi

  case "$existing_path" in
    /opt/homebrew/* | /usr/local/*)
      if [ "$os" = "darwin" ]; then
        printf 'brew\n'
        return 0
      fi
      ;;
  esac

  if [ -f "$existing_path" ] && grep -F "#!/usr/bin/env node" "$existing_path" >/dev/null 2>&1; then
    case "$existing_path" in
      *".bun"*)
        printf 'bun\n'
        ;;
      *)
        printf 'npm\n'
        ;;
    esac
    return 0
  fi

  return 1
}

prompt_yes_no() {
  prompt="$1"

  case "$NON_INTERACTIVE" in
    1 | [Tt][Rr][Uu][Ee] | [Yy][Ee][Ss])
      return 1
      ;;
  esac

  if ( : </dev/tty ) 2>/dev/null; then
    printf '%s [y/N] ' "$prompt" >/dev/tty
    if ! IFS= read -r answer </dev/tty; then
      return 1
    fi
  elif [ -t 0 ]; then
    printf '%s [y/N] ' "$prompt"
    if ! IFS= read -r answer; then
      return 1
    fi
  else
    return 1
  fi

  case "$answer" in
    y | Y | yes | YES)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

print_launch_instructions() {
  case "$path_action" in
    added)
      step "Current terminal: export PATH=\"$BIN_DIR:\$PATH\" && codex"
      step "Future terminals: open a new terminal and run: codex"
      step "PATH was added to $path_profile"
      ;;
    updated)
      step "Current terminal: export PATH=\"$BIN_DIR:\$PATH\" && codex"
      step "Future terminals: open a new terminal and run: codex"
      step "PATH was updated in $path_profile"
      ;;
    configured)
      step "Current terminal: export PATH=\"$BIN_DIR:\$PATH\" && codex"
      step "Future terminals: open a new terminal and run: codex"
      step "PATH is already configured in $path_profile"
      ;;
    *)
      step "Current terminal: codex"
      step "Future terminals: open a new terminal and run: codex"
      ;;
  esac
}

maybe_launch_codex_now() {
  if prompt_yes_no "Start Codex now?"; then
    step "Launching Codex"
    "$BIN_PATH"
  fi
}

detect_conflicting_install() {
  existing_path="$(resolve_existing_codex)"
  manager="$(classify_existing_codex "$existing_path" || true)"

  if [ -z "$manager" ]; then
    return
  fi

  conflict_manager="$manager"
  step "Detected existing $manager-managed Codex at $existing_path"
  warn "Multiple managed Codex installs can be ambiguous because PATH order decides which one runs."
}

handle_conflicting_install() {
  if [ -z "$conflict_manager" ]; then
    return
  fi

  case "$conflict_manager" in
    brew)
      uninstall_cmd="brew uninstall --cask codex"
      ;;
    bun)
      uninstall_cmd="bun remove -g @openai/codex"
      ;;
    *)
      uninstall_cmd="npm uninstall -g @openai/codex"
      ;;
  esac

  if prompt_yes_no "Uninstall the existing $conflict_manager-managed Codex now?"; then
    step "Running: $uninstall_cmd"
    if ! sh -c "$uninstall_cmd"; then
      warn "Failed to uninstall the existing $conflict_manager-managed Codex. Continuing with the standalone install."
    fi
  else
    warn "Leaving the existing $conflict_manager-managed Codex installed. PATH order will determine which codex runs."
  fi
}

validate_package_manifest() {
  manifest_path="$1"
  expected_version="$2"
  expected_target="$3"

  [ -f "$manifest_path" ] && [ ! -L "$manifest_path" ] &&
    [ "$(wc -c <"$manifest_path")" -le 4096 ] || return 1
  # The canonical manifest is a small flat object with literal names/values.
  # Parse its grammar rather than accepting matching fields in malformed JSON.
  LC_ALL=C awk -v version="$expected_version" -v target="$expected_target" '
    function skip_space() { sub(/^[ \t\r\n]*/, "", rest) }
    function take_string( token) {
      skip_space()
      if (!match(rest, /^"[^"\\]*"/)) { invalid = 1; return "" }
      token = substr(rest, 2, RLENGTH - 2)
      rest = substr(rest, RLENGTH + 1)
      return token
    }
    { document = document $0 "\n" }
    END {
      expected["version"] = version
      expected["target"] = target
      expected["variant"] = "codex"
      expected["entrypoint"] = "bin/codex"
      expected["resourcesDir"] = "codex-resources"
      expected["pathDir"] = "codex-path"
      rest = document
      skip_space()
      if (substr(rest, 1, 1) != "{") exit 1
      rest = substr(rest, 2)
      while (!invalid) {
        key = take_string()
        if (invalid || seen[key]++) exit 1
        skip_space()
        if (substr(rest, 1, 1) != ":") exit 1
        rest = substr(rest, 2)
        skip_space()
        if (key == "layoutVersion") {
          if (substr(rest, 1, 1) != "1") exit 1
          rest = substr(rest, 2)
        } else {
          value = take_string()
          if (!(key in expected) || value != expected[key]) exit 1
        }
        skip_space()
        separator = substr(rest, 1, 1)
        rest = substr(rest, 2)
        if (separator == "}") break
        if (separator != ",") exit 1
      }
      skip_space()
      if (invalid || rest != "" || !seen["layoutVersion"]) exit 1
      for (key in expected) if (!seen[key]) exit 1
    }
  ' "$manifest_path"
}

package_files_are_complete() {
  package_dir="$1"
  expected_version="$2"
  expected_target="$3"

  for directory in bin codex-path codex-resources; do
    [ -d "$package_dir/$directory" ] &&
      [ ! -L "$package_dir/$directory" ] || return 1
  done
  executable_paths="bin/codex bin/codex-code-mode-host codex-path/rg"
  if [ "$expected_target" = "x86_64-unknown-linux-gnu" ]; then
    executable_paths="$executable_paths codex-resources/bwrap"
  fi
  for executable in $executable_paths; do
    [ -f "$package_dir/$executable" ] &&
      [ ! -L "$package_dir/$executable" ] &&
      [ -x "$package_dir/$executable" ] || return 1
  done
  validate_package_manifest "$package_dir/codex-package.json" "$expected_version" "$expected_target" &&
    [ "$(version_from_binary "$package_dir/bin/codex")" = "$expected_version" ]
}

install_fork_release() {
  release_dir="$1"
  archive_path="$2"
  stage_release="$RELEASES_DIR/.staging.$(basename "$release_dir").$$"

  # Check names AND member types before extraction: even an allowed path could
  # otherwise be a link or device that writes outside the staging directory.
  tar -tzf "$archive_path" >"$tmp_dir/archive-members" || return 1
  tar -tvzf "$archive_path" >"$tmp_dir/archive-types" || return 1
  if ! LC_ALL=C awk -v target="$vendor_target" '
    BEGIN {
      required["bin/"] = "d"
      required["codex-path/"] = "d"
      required["codex-resources/"] = "d"
      required["codex-package.json"] = "-"
      required["bin/codex"] = "-"
      required["bin/codex-code-mode-host"] = "-"
      required["codex-path/rg"] = "-"
      if (target == "x86_64-unknown-linux-gnu") required["codex-resources/bwrap"] = "-"
    }
    NR == FNR {
      if (!($0 in required) || members[$0]++) invalid = 1
      next
    }
    {
      name = $NF
      if (!(name in required) || types[name]++ ||
          substr($1, 1, 1) != required[name]) invalid = 1
    }
    END {
      for (name in required) if (members[name] != 1 || types[name] != 1) invalid = 1
      exit invalid
    }
  ' "$tmp_dir/archive-members" "$tmp_dir/archive-types"; then
    echo "ASM archive must contain a complete canonical package with regular files only. Reinstall a newer ASM release; old two-binary releases require their original installer." >&2
    return 1
  fi
  mkdir -p "$RELEASES_DIR"
  rm -rf "$stage_release"
  mkdir "$stage_release"
  tar -xzf "$archive_path" -C "$stage_release"
  if ! package_files_are_complete "$stage_release" "$resolved_version" "$vendor_target"; then
    echo "Invalid ASM package metadata or required executables." >&2
    rm -rf "$stage_release"
    return 1
  fi
  ln -s "bin/codex" "$stage_release/codex"
  if [ -e "$release_dir" ] || [ -L "$release_dir" ]; then
    rm -rf "$release_dir"
  fi
  mv "$stage_release" "$release_dir"
}

release_dir_is_complete() {
  release_dir="$1"
  expected_version="$2"
  expected_target="$3"

  [ -d "$release_dir" ] && [ ! -L "$release_dir" ] &&
    [ "$(basename "$release_dir")" = "$expected_version-$expected_target" ] ||
    return 1
  package_files_are_complete "$release_dir" "$expected_version" "$expected_target"
}

update_current_link() {
  release_dir="$1"
  tmp_link="$STANDALONE_ROOT/.current.$$"

  replace_path_with_symlink "$CURRENT_LINK" "$release_dir" "$tmp_link"
}

update_visible_command() {
  mkdir -p "$BIN_DIR"
  tmp_link="$BIN_DIR/.codex.$$"

  replace_path_with_symlink "$BIN_PATH" "$CURRENT_LINK/bin/codex" "$tmp_link"

  replace_path_with_symlink \
    "$CODE_MODE_HOST_BIN_PATH" \
    "$CURRENT_LINK/bin/codex-code-mode-host" \
    "$BIN_DIR/.codex-code-mode-host.$$"
}

verify_visible_command() {
  "$BIN_PATH" --version >/dev/null
  [ -x "$CODE_MODE_HOST_BIN_PATH" ]
}

parse_args "$@"
if [ "$DAEMON_ONLY" = "1" ] || [ "${CODEX_INSTALL_DEFER_SELECTION:-0}" = "1" ] ||
  [ "${CODEX_INSTALL_IF_LATEST:-0}" = "1" ] || [ "${CODEX_INSTALL_IF_CURRENT:-0}" = "1" ]; then
  echo "ASM installer does not support daemon-only or automatic updater modes." >&2
  exit 1
fi

require_command mktemp
require_command tar

case "$(uname -s)" in
  Darwin)
    os="darwin"
    ;;
  Linux)
    os="linux"
    ;;
  *)
    echo "ASM installer supports macOS Apple Silicon and Linux x86_64 only." >&2
    exit 1
    ;;
esac

machine="$(uname -m)"
if [ "$os" = "darwin" ] && [ "$machine" = "x86_64" ] &&
  [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = "1" ]; then
  machine="arm64"
fi

case "$os:$machine" in
  darwin:arm64 | darwin:aarch64)
    vendor_target="aarch64-apple-darwin"
    platform_label="macOS (Apple Silicon)"
    ;;
  linux:x86_64 | linux:amd64)
    require_linux_glibc
    vendor_target="x86_64-unknown-linux-gnu"
    platform_label="Linux (x64, glibc 2.35+)"
    ;;
  *)
    echo "Unsupported ASM release target: $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

resolve_release
release_name="$resolved_version-$vendor_target"
release_dir="$RELEASES_DIR/$release_name"
current_version="$(current_installed_version)"

if [ -n "$current_version" ] && [ "$current_version" != "$resolved_version" ]; then
  step "Updating Codex CLI from $current_version to $resolved_version"
elif [ -n "$current_version" ]; then
  step "Updating Codex CLI"
else
  step "Installing Codex CLI"
fi
step "Detected platform: $platform_label"
step "Resolved version: $resolved_version"

detect_conflicting_install

tmp_dir="$(mktemp -d)"
cleanup() {
  release_install_lock
  if [ -n "$tmp_dir" ]; then
    rm -rf "$tmp_dir"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

acquire_install_lock
cleanup_stale_install_artifacts

if ! release_dir_is_complete "$release_dir" "$resolved_version" "$vendor_target"; then
  if [ -e "$release_dir" ] || [ -L "$release_dir" ]; then
    warn "Found incomplete existing release at $release_dir; reinstalling."
  fi

  archive_path="$tmp_dir/$asset"
  checksum_path="$tmp_dir/$checksum_asset"

  step "Downloading complete ASM package"
  checksum_digest="$(release_asset_digest "$checksum_asset")"
  download_file "$checksum_url" "$checksum_path"
  verify_archive_digest "$checksum_path" "$checksum_digest"
  if [ "$checksum_asset" != SHA256SUMS ]; then
    if ! awk -v archive="$asset" '
      NF != 2 || length($1) != 64 || $1 ~ /[^0-9a-fA-F]/ ||
        ($2 != archive && $2 != "install.sh") || seen[$2]++ { invalid = 1 }
      END { exit invalid || NR != 2 || !seen[archive] || !seen["install.sh"] }
    ' "$checksum_path"; then
      echo "Invalid platform checksum manifest: $checksum_asset." >&2
      exit 1
    fi
    installer_digest="$(package_archive_digest install.sh "$checksum_path")"
    [ "$installer_digest" = "$(release_asset_digest install.sh)" ] || {
      echo "ASM installer metadata and platform checksum manifest disagree." >&2
      exit 1
    }
  fi
  expected_digest="$(package_archive_digest "$asset" "$checksum_path")"
  release_digest="$(release_asset_digest "$asset")"
  [ "$expected_digest" = "$release_digest" ] || {
    echo "ASM release metadata and SHA256SUMS disagree." >&2
    exit 1
  }
  download_file "$download_url" "$archive_path"
  verify_archive_digest "$archive_path" "$expected_digest"

  step "Installing ASM package to $release_dir"
  install_fork_release "$release_dir" "$archive_path"
fi
if ! release_dir_is_complete "$release_dir" "$resolved_version" "$vendor_target"; then
  echo "Installed Codex command did not report expected version $resolved_version." >&2
  exit 1
fi
update_current_link "$release_dir"
# Do not create auto-update-version: inherited runtime updaters target OpenAI.
update_visible_command
add_to_path
verify_visible_command
release_install_lock
handle_conflicting_install

case "$path_action" in
  added)
    print_launch_instructions
    ;;
  updated)
    print_launch_instructions
    ;;
  configured)
    print_launch_instructions
    ;;
  *)
    step "$BIN_DIR is already on PATH"
    print_launch_instructions
    ;;
esac

printf 'ASM CLI %s installed successfully.\n' "$resolved_version"
step "Use this ASM installer for manual updates; the inherited in-app updater targets OpenAI."
step "The package includes rg and required platform resources; use a system shell."
maybe_launch_codex_now
