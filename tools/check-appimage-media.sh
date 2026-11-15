#!/usr/bin/env bash
set -euo pipefail

# Exercise the packaged launcher without opening Rustory or contacting a device.
# Only the executable in a disposable extraction is replaced with the probe.
if [[ $# -ne 1 || ! -f "$1" ]]; then
  echo "Usage: $0 path/to/Rustory.AppImage" >&2
  exit 2
fi

for tool in cc pkg-config python3 realpath mktemp timeout; do
  command -v "$tool" >/dev/null || { echo "Missing tool: $tool" >&2; exit 2; }
done
pkg-config --exists gstreamer-1.0 || {
  echo "Install the GStreamer development package (libgstreamer1.0-dev)." >&2
  exit 2
}

artifact=$(realpath "$1")
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/rustory-appimage-media.XXXXXX")
trap 'rm -rf -- "$scratch"' EXIT

# No FUSE mount, modification of the signed artifact, or use of the user's data.
(cd "$scratch" && "$artifact" --appimage-extract >extract.log)
appdir="$scratch/squashfs-root"
[[ -x "$appdir/AppRun" && -f "$appdir/usr/bin/rustory" ]] || {
  echo "The AppImage does not contain the expected Rustory launcher." >&2
  exit 1
}

read -r -a gst_flags <<<"$(pkg-config --cflags --libs gstreamer-1.0)"
cc -std=c11 -Wall -Wextra -Werror "$script_dir/appimage-media-probe.c" \
  -o "$scratch/media-probe" "${gst_flags[@]}" -ldl
cp -- "$scratch/media-probe" "$appdir/usr/bin/rustory"

python3 - "$scratch/input.wav" <<'PY'
import math
import struct
import sys
import wave

with wave.open(sys.argv[1], "wb") as audio:
    audio.setnchannels(1)
    audio.setsampwidth(2)
    audio.setframerate(8000)
    audio.writeframes(b"".join(
        struct.pack("<h", round(8192 * math.sin(2 * math.pi * 440 * sample / 8000)))
        for sample in range(800)
    ))
PY

mkdir -p "$scratch/home" "$scratch/config" "$scratch/data" "$scratch/cache" "$scratch/runtime"
chmod 700 "$scratch/runtime"
for cache_state in cold warm; do
  echo "Checking AppImage media with a $cache_state registry: $artifact"
  # Do not set GST_PLUGIN_* or LD_LIBRARY_PATH here: only the real AppRun and
  # its shipped hooks may select libraries, plugins and the plugin scanner.
  # The AppImage runtime normally supplies APPDIR before any hook runs. An
  # extracted AppRun needs the same value, regardless of the hook ordering.
  if ! timeout 45s env -i \
    PATH=/usr/bin:/bin \
    APPDIR="$appdir" \
    HOME="$scratch/home" \
    XDG_CONFIG_HOME="$scratch/config" \
    XDG_DATA_HOME="$scratch/data" \
    XDG_CACHE_HOME="$scratch/cache" \
    XDG_RUNTIME_DIR="$scratch/runtime" \
    GSETTINGS_BACKEND=memory \
    GST_DEBUG=GST_REGISTRY:2 \
    GST_DEBUG_NO_COLOR=1 \
    LANG=C \
    "$appdir/AppRun" "$appdir" "$scratch/input.wav" \
    >"$scratch/$cache_state.log" 2>&1; then
    cat "$scratch/$cache_state.log" >&2
    echo "AppImage media check failed ($cache_state registry)." >&2
    exit 1
  fi
  cat "$scratch/$cache_state.log"
  if grep -Eiq 'external plugin loader failed|failed starting plugin scanner' "$scratch/$cache_state.log"; then
    echo "The packaged GStreamer scanner failed ($cache_state registry)." >&2
    exit 1
  fi
  if [[ "$cache_state" == cold ]]; then
    registries=("$scratch/cache/gstreamer-1.0"/registry.*.bin)
    if [[ ! -s "${registries[0]}" ]]; then
      echo "GStreamer did not create a registry in the temporary cache." >&2
      exit 1
    fi
  fi
done

echo "AppImage media check passed with cold and warm registries."
