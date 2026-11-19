#!/usr/bin/env bash
# User-local source install/update. Project configuration and state are never touched.
set -Eeuo pipefail

greenlight_pending_binary=''
greenlight_install_lock=''
cleanup_install() {
    if [[ -n "$greenlight_pending_binary" ]]; then rm -f -- "$greenlight_pending_binary"; fi
    if [[ -n "$greenlight_install_lock" ]]; then rmdir -- "$greenlight_install_lock" 2>/dev/null || true; fi
}
fail() { printf 'greenlight installer: %s\n' "$*" >&2; exit 1; }
usage() {
    cat <<'EOF'
Usage: bash install.sh [--bin-dir DIR] [--data-dir DIR] [--ref REF] [--repository URL]

Fetch, build and install Greenlight; rerunning upgrades an existing installation.
Default source: https://github.com/AlexAuragan/Greenlight.git, master on first install.
Default cache: ${XDG_DATA_HOME:-$HOME/.local/share}/greenlight
Default binary: previous install location, an existing user-local greenlight, or ~/.local/bin.
The selected source/ref/location is remembered for subsequent updates.

Requires Linux, Git, Cargo/Rust and a C compiler. No sudo or shell-profile edits.
An update never overwrites project .greenlight settings, workflows or history.
EOF
}

main() {
    umask 077
    local install_data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/greenlight"
    local install_bin_dir='' install_repository='' install_ref=''
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --bin-dir|--data-dir|--ref|--repository)
                [[ $# -ge 2 && -n "$2" && "$2" != --* ]] || fail "Missing value for $1"
                case "$1" in
                    --bin-dir) install_bin_dir=$2 ;;
                    --data-dir) install_data_dir=$2 ;;
                    --ref) install_ref=$2 ;;
                    --repository) install_repository=$2 ;;
                esac
                shift 2 ;;
            --help|-h) usage; return ;;
            *) fail "Unknown argument: $1 (use --help)" ;;
        esac
    done
    [[ $(uname -s) == Linux ]] || fail 'Linux is required for the current shell sandbox.'
    local dependency
    for dependency in git cargo rustc cc install mktemp; do
        command -v "$dependency" >/dev/null 2>&1 || fail "Missing $dependency. Install Git, a current stable Rust toolchain and a C compiler, then rerun."
    done
    [[ "$install_data_dir" != / && "$install_data_dir" != "$HOME" ]] || fail 'Choose a dedicated --data-dir.'
    mkdir -p -- "$install_data_dir"
    install_data_dir=$(cd -- "$install_data_dir" && pwd -P)
    [[ "$install_data_dir" != / && "$install_data_dir" != "$HOME" ]] || fail 'Choose a dedicated --data-dir.'
    [[ ! -L "$install_data_dir/install.lock" ]] || fail 'Unsafe install lock path.'
    mkdir -- "$install_data_dir/install.lock" 2>/dev/null || fail "Another install is active (or an interrupted install left $install_data_dir/install.lock)."
    greenlight_install_lock="$install_data_dir/install.lock"
    trap cleanup_install EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    local source_dir="$install_data_dir/source"
    local expected_repository='' previous_ref='' expected_revision='' previous_binary=''
    if [[ -f "$install_data_dir/repository" ]]; then expected_repository=$(cat -- "$install_data_dir/repository"); fi
    if [[ -f "$install_data_dir/ref" ]]; then previous_ref=$(cat -- "$install_data_dir/ref"); fi
    if [[ -f "$install_data_dir/source-revision" ]]; then expected_revision=$(cat -- "$install_data_dir/source-revision"); fi
    if [[ -f "$install_data_dir/bin-dir" ]]; then previous_binary=$(cat -- "$install_data_dir/bin-dir"); fi
    install_repository=${install_repository:-${expected_repository:-https://github.com/AlexAuragan/Greenlight.git}}
    install_ref=${install_ref:-${previous_ref:-master}}
    case "$install_ref" in -*|*[[:space:]]*) fail 'Invalid ref; use a branch, tag or commit ID.' ;; esac
    if [[ -z "$install_bin_dir" ]]; then
        if [[ -n "$previous_binary" ]]; then install_bin_dir=$previous_binary
        else
            local existing
            existing=$(command -v greenlight || true)
            if [[ "$existing" == "$HOME/"* && -f "$existing" && -w $(dirname -- "$existing") ]]; then
                install_bin_dir=$(dirname -- "$existing")
            else install_bin_dir="$HOME/.local/bin"
            fi
        fi
    fi
    [[ "$install_bin_dir" != / ]] || fail 'Choose a binary directory, not /.'
    mkdir -p -- "$install_bin_dir"
    install_bin_dir=$(cd -- "$install_bin_dir" && pwd -P)
    [[ "$install_bin_dir" != / ]] || fail 'Choose a binary directory, not /.'

    if [[ -e "$source_dir" || -L "$source_dir" ]]; then
        [[ ! -L "$source_dir" && -d "$source_dir/.git" && -n "$expected_repository" && -n "$expected_revision" ]] || fail 'Source cache is not an intact managed checkout; choose a fresh --data-dir.'
        [[ "$install_repository" == "$expected_repository" ]] || fail 'Repository differs from the managed installation; choose a separate --data-dir.'
        [[ $(git -C "$source_dir" config --get remote.origin.url) == "$expected_repository" ]] || fail 'Managed origin changed; refusing to overwrite it.'
        [[ $(git -C "$source_dir" rev-parse HEAD) == "$expected_revision" ]] || fail 'Managed checkout has local commits; preserve them or choose another --data-dir.'
        [[ -z $(git -C "$source_dir" status --porcelain --untracked-files=all) ]] || fail 'Managed checkout has local changes; preserve them or choose another --data-dir.'
    else
        if [[ -z "$expected_repository" ]]; then
            local entry
            for entry in "$install_data_dir"/* "$install_data_dir"/.[!.]*; do
                [[ -e "$entry" || -L "$entry" ]] || continue
                [[ "$entry" == "$install_data_dir/install.lock" ]] || fail 'Data directory is not empty or managed; choose a dedicated --data-dir.'
            done
        fi
        printf 'Cloning Greenlight source…\n'
        git clone --no-tags -- "$install_repository" "$source_dir"
        printf '%s\n' "$install_repository" > "$install_data_dir/repository"
        expected_revision=$(git -C "$source_dir" rev-parse HEAD)
        printf '%s\n' "$expected_revision" > "$install_data_dir/source-revision"
    fi
    printf 'Fetching %s…\n' "$install_ref"
    git -C "$source_dir" fetch --no-tags -- origin "$install_ref"
    local revision
    revision=$(git -C "$source_dir" rev-parse --verify 'FETCH_HEAD^{commit}')
    if [[ "$previous_ref" == "$install_ref" ]] && ! git -C "$source_dir" merge-base --is-ancestor HEAD "$revision"; then
        fail 'Upstream ref moved backwards/diverged; refusing an automatic downgrade. Choose a separate data directory to inspect it.'
    fi
    git -C "$source_dir" checkout --detach "$revision"
    printf '%s\n' "$revision" > "$install_data_dir/source-revision"
    printf '%s\n' "$install_ref" > "$install_data_dir/ref"

    local host build_dir built_binary
    host=$(rustc -vV | sed -n 's/^host: //p')
    [[ -n "$host" ]] || fail 'Cannot determine the native Rust target.'
    build_dir="$install_data_dir/build"
    printf 'Building release %s…\n' "${revision:0:12}"
    GREENLIGHT_BUILD_COMMIT="${revision:0:12}" cargo build --locked --release --target "$host" --target-dir "$build_dir" --manifest-path "$source_dir/Cargo.toml"
    built_binary="$build_dir/$host/release/greenlight"
    [[ -f "$built_binary" && -x "$built_binary" ]] || fail 'Cargo did not produce the expected native executable.'
    greenlight_pending_binary=$(mktemp "$install_bin_dir/.greenlight.XXXXXXXX")
    install -m 755 -- "$built_binary" "$greenlight_pending_binary"
    "$greenlight_pending_binary" --version
    "$greenlight_pending_binary" init --help >/dev/null 2>&1 || fail 'Selected ref does not support project initialization; choose a ref containing the installer/project-layout changes.'
    mv -f -- "$greenlight_pending_binary" "$install_bin_dir/greenlight"
    greenlight_pending_binary=''
    printf '%s\n' "$install_bin_dir" > "$install_data_dir/bin-dir"
    printf '%s\n' "$revision" > "$install_data_dir/installed-revision"
    printf '\nInstalled/updated: %s/greenlight\n' "$install_bin_dir"
    printf 'Revision: %s\nRerun this installer to fetch and upgrade the same ref. Restart running Greenlight processes after updating.\n' "$revision"
    local selected
    selected=$(command -v greenlight || true)
    if [[ "$selected" != "$install_bin_dir/greenlight" ]]; then
        printf '\nTo use this installation in the current shell (and add to your shell profile if needed):\n'
        printf '  export PATH=%q:"$PATH"\n  hash -r\n' "$install_bin_dir"
        if [[ -n "$selected" ]]; then printf 'Current PATH resolves greenlight to: %s\n' "$selected"; fi
    fi
    [[ -x /usr/bin/bwrap ]] || printf '\nShell workflows require /usr/bin/bwrap; install Bubblewrap before using shell tools.\n'
    printf '\nIn any project: greenlight init, then greenlight (or greenlight tui).\n'
}
# Parse the complete implementation before updating a checkout that contains this script.
main "$@"; exit "$?"
