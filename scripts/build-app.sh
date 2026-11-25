#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

DERIVED="${ROOT}/build/DerivedData"
echo "Building Packager"
xcodebuild \
  -project "${ROOT}/Packager.xcodeproj" \
  -scheme Packager \
  -configuration Release \
  -derivedDataPath "${DERIVED}" \
  build

APP="${DERIVED}/Build/Products/Release/Packager.app"
if [[ ! -d "${APP}" ]]; then
  echo "error: Xcode did not produce ${APP}" >&2
  exit 1
fi

mkdir -p "${ROOT}/dist"
rm -rf "${ROOT}/dist/Packager.app"
cp -R "${APP}" "${ROOT}/dist/Packager.app"
codesign --force --sign - "${ROOT}/dist/Packager.app/Contents/MacOS/packager-engine"
codesign --force --sign - "${ROOT}/dist/Packager.app"

echo "Built ${ROOT}/dist/Packager.app"
