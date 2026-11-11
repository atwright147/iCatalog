#!/usr/bin/env bash
# Emit document registration only for app source that can receive Finder events.
set -euo pipefail

manifest=${1:?Usage: macos-document-types.sh <icatalog-ui Cargo.toml>}
if ! grep -q '^fmv-macos-events = ' "$manifest"; then
  exit 0
fi
cat <<'EOF'
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeName</key><string>iCatalog Catalogue</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSHandlerRank</key><string>Owner</string>
    <key>LSItemContentTypes</key><array><string>org.icatalog.catalogue</string></array>
  </dict></array>
  <key>UTExportedTypeDeclarations</key><array><dict>
    <key>UTTypeIdentifier</key><string>org.icatalog.catalogue</string>
    <key>UTTypeDescription</key><string>iCatalog Catalogue</string>
    <key>UTTypeConformsTo</key><array><string>public.data</string></array>
    <key>UTTypeTagSpecification</key><dict>
      <key>public.filename-extension</key><array><string>icatalog</string></array>
      <key>public.mime-type</key><string>application/x-icatalog</string>
    </dict>
  </dict></array>
EOF
