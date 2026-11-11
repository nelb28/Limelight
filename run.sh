#!/usr/bin/env bash
# Limelight helper: convert audio, run CWT processing, and play the visualizer.
#
# Layout:  Audio Processing/Music/<album>/<song>.{flac,mp3,wav,ogg}
#          Audio Processing/PCM/<album>/<song>.{pcm,sr,cwt}
#
# A <song> can be: a path to the audio/pcm/cwt file, a path relative to Music/
# (with or without extension, e.g. "Spectrum/Stratus"), or just a unique
# case-insensitive piece of the name (e.g. "stratus").
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AP="$ROOT/Audio Processing"
MUSIC="$AP/Music"
PCM="$AP/PCM"
CWT_BIN="$ROOT/cwt/target/release/cwt"
VIS_BIN="$ROOT/music_vis/target/release/music_vis"

usage() {
  cat <<USAGE
Usage: ./run.sh <command> [args]

Commands:
  build                     Build cwt and music_vis (release)
  convert [music] [pcm]     Convert audio to PCM + .sr (skips already converted)
  cwt  <song> [--force]     Run CWT processing on one song (skips if .cwt exists unless --force)
  play <song>               Open the visualizer for one song (runs cwt first if needed)
  list                      List songs: [cwt] processed, [old] needs reprocessing for phase
USAGE
  exit 1
}

# Rebuild a crate if its binary is missing or older than any source file.
ensure_built() {
  local dir="$1" bin="$2"
  if [[ ! -x "$bin" ]] || [[ -n "$(find "$dir/src" "$dir/Cargo.toml" -newer "$bin" -print -quit)" ]]; then
    echo ">> Building $(basename "$dir")..."
    (cd "$dir" && cargo build --release)
  fi
}

# Sets REL (album/song, no extension) and AUDIO (audio file path, may be empty).
resolve_song() {
  local q="$1" rel
  q="${q#./}"
  if [[ -e "$q" ]]; then q="$(cd "$(dirname "$q")" && pwd)/$(basename "$q")"; fi
  rel="${q#"$MUSIC"/}"; rel="${rel#"$PCM"/}"
  rel="${rel%.*}"
  # Exact match under Music or PCM?
  local cand
  for cand in "$MUSIC/$rel".{flac,mp3,wav,ogg}; do
    [[ -f "$cand" ]] && { REL="$rel"; AUDIO="$cand"; return; }
  done
  # Fuzzy search by name
  local matches=()
  while IFS= read -r f; do matches+=("$f"); done < <(
    find "$MUSIC" -type f \( -iname '*.flac' -o -iname '*.mp3' -o -iname '*.wav' -o -iname '*.ogg' \) \
      | grep -iF -- "$1" | sort)
  if [[ ${#matches[@]} -eq 1 ]]; then
    AUDIO="${matches[0]}"; REL="${AUDIO#"$MUSIC"/}"; REL="${REL%.*}"; return
  elif [[ ${#matches[@]} -eq 0 ]]; then
    echo "No song matching '$1' under $MUSIC" >&2
  else
    echo "'$1' is ambiguous; matches:" >&2
    printf '  %s\n' "${matches[@]#"$MUSIC"/}" >&2
  fi
  exit 1
}

run_cwt() {
  local force="${1:-}"
  local pcm="$PCM/$REL.pcm" cwt="$PCM/$REL.cwt"
  if [[ ! -f "$pcm" ]]; then
    echo "No PCM for '$REL'. Run: ./run.sh convert" >&2; exit 1
  fi
  if [[ -f "$cwt" && "$force" != "--force" ]]; then
    if [[ "$(head -c 4 "$cwt")" == "CWT2" ]]; then
      echo ">> $REL.cwt already exists (use --force to redo)"; return
    fi
    echo ">> $REL.cwt is an old CWT1 file without phase data; regenerating"
  fi
  if [[ ! -f "$PCM/$REL.sr" ]]; then
    echo "warning: no .sr file for '$REL'; cwt will assume 96000 Hz" >&2
  fi
  ensure_built "$ROOT/cwt" "$CWT_BIN"
  "$CWT_BIN" "$pcm" "$cwt"
}

cmd="${1:-}"; [[ $# -gt 0 ]] && shift
case "$cmd" in
  build)
    (cd "$ROOT/cwt" && cargo build --release)
    (cd "$ROOT/music_vis" && cargo build --release)
    ;;
  convert)
    python3 "$AP/pcm_converter.py" "${1:-$MUSIC}" "${2:-$PCM}"
    ;;
  cwt)
    [[ $# -ge 1 ]] || usage
    resolve_song "$1"; run_cwt "${2:-}"
    ;;
  play)
    [[ $# -ge 1 ]] || usage
    resolve_song "$1"; run_cwt
    ensure_built "$ROOT/music_vis" "$VIS_BIN"
    "$VIS_BIN" "$AUDIO" "$PCM/$REL.cwt"
    ;;
  list)
    find "$MUSIC" -type f \( -iname '*.flac' -o -iname '*.mp3' -o -iname '*.wav' -o -iname '*.ogg' \) | sort |
    while IFS= read -r f; do
      r="${f#"$MUSIC"/}"; r="${r%.*}"
      if [[ ! -f "$PCM/$r.cwt" ]]; then mark="[   ]"
      elif [[ "$(head -c 4 "$PCM/$r.cwt")" == "CWT2" ]]; then mark="[cwt]"
      else mark="[old]"; fi
      echo "$mark $r"
    done
    ;;
  *) usage ;;
esac
