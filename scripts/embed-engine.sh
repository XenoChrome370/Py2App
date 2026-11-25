#!/bin/bash
# Xcode build phase: compile the Rust engine and copy it into the app bundle.
set -euo pipefail

ROOT="${SRCROOT}"

export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:${PATH:-/usr/bin:/bin}"

if ! command -v cargo >/dev/null 2>&1; then
  if [[ -x /bin/zsh ]]; then
    login_path="$(/bin/zsh -lc 'printf %s "$PATH"' || true)"
    if [[ -n "${login_path}" ]]; then
      export PATH="${login_path}:${PATH}"
    fi
  fi
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo was not found. Install Rust, then build again." >&2
  exit 1
fi

case "${CONFIGURATION}" in
  Release)
    profile="release"
    cargo_args=(--release)
    ;;
  *)
    profile="debug"
    cargo_args=()
    ;;
esac

cargo build "${cargo_args[@]}" --manifest-path "${ROOT}/rust/Cargo.toml"

target_dir="$(
  cargo metadata \
    --manifest-path "${ROOT}/rust/Cargo.toml" \
    --format-version 1 \
    --no-deps \
    --offline \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])'
)"
engine="${target_dir}/${profile}/packager"

if [[ ! -x "${engine}" ]]; then
  echo "error: packaging engine was not produced at ${engine}" >&2
  exit 1
fi

dest="${BUILT_PRODUCTS_DIR}/${EXECUTABLE_FOLDER_PATH}"
mkdir -p "${dest}"
cp "${engine}" "${dest}/packager-engine"
chmod +x "${dest}/packager-engine"

if [[ "${CODE_SIGNING_ALLOWED:-YES}" != "NO" ]]; then
  identity="${EXPANDED_CODE_SIGN_IDENTITY:--}"
  if [[ -z "${identity}" ]]; then
    identity="-"
  fi
  codesign --force --sign "${identity}" "${dest}/packager-engine"
fi
