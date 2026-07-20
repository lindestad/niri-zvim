#!/usr/bin/env bash
set -euo pipefail

repository="https://github.com/lindestad/niri-zvim"
version="${NIRI_ZVIM_INSTALL_VERSION:-}"
release_base="${NIRI_ZVIM_INSTALL_RELEASE_BASE:-}"
install_arguments=()

while (($# > 0)); do
  case "$1" in
    --version)
      (($# >= 2)) || {
        echo "--version requires a value" >&2
        exit 2
      }
      version="$2"
      shift 2
      ;;
    --no-service | --replace-hjkl-binds)
      install_arguments+=("$1")
      shift
      ;;
    -h | --help)
      echo "usage: install.sh [--version VERSION] [--no-service] [--replace-hjkl-binds]"
      exit 0
      ;;
    *)
      echo "usage: install.sh [--version VERSION] [--no-service] [--replace-hjkl-binds]" >&2
      exit 2
      ;;
  esac
done

for command in curl mktemp sha256sum tar uname; do
  command -v "$command" >/dev/null || {
    echo "$command is required" >&2
    exit 1
  }
done

case "$(uname -s):$(uname -m)" in
  Linux:x86_64) target="x86_64-unknown-linux-gnu" ;;
  *)
    echo "no prebuilt niri-zvim release for $(uname -s) $(uname -m); install from source" >&2
    exit 1
    ;;
esac

curl_arguments=(--fail --location --silent --show-error)
if [[ -z "$version" ]]; then
  latest_url="$(curl "${curl_arguments[@]}" \
    --output /dev/null \
    --write-out '%{url_effective}' \
    "$repository/releases/latest")"
  tag="${latest_url##*/}"
else
  tag="v${version#v}"
fi
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "could not determine a release version (found: $tag)" >&2
  exit 1
fi
version="${tag#v}"
if [[ -z "$release_base" ]]; then
  release_base="$repository/releases/download/$tag"
fi

archive="niri-zvim-$version-$target.tar.gz"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/niri-zvim-bootstrap.XXXXXX")"
cleanup() {
  rm -rf -- "$temporary"
}
trap cleanup EXIT

curl "${curl_arguments[@]}" --output "$temporary/$archive" "$release_base/$archive"
curl "${curl_arguments[@]}" \
  --output "$temporary/$archive.sha256" \
  "$release_base/$archive.sha256"
(
  cd "$temporary"
  sha256sum --check "$archive.sha256"
)
tar --directory "$temporary" --extract --gzip --file "$temporary/$archive"
bundle="$temporary/niri-zvim-$version-$target"
[[ -x "$bundle/install" ]] || {
  echo "release archive does not contain an installer" >&2
  exit 1
}
"$bundle/install" "${install_arguments[@]}"
