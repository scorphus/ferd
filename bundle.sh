#!/bin/sh
set -e
cd "$(dirname "$0")"
cargo build --release
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=target/Ferd.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/ferd "$app/Contents/MacOS/Ferd"
set_dir=target/Ferd.iconset
rm -rf "$set_dir"
mkdir "$set_dir"
for s in 16 32 128 256 512; do
    sips -z $s $s assets/icon.png --out "$set_dir/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) assets/icon.png --out "$set_dir/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o "$app/Contents/Resources/Ferd.icns"
rm -rf "$set_dir"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Ferd</string>
    <key>CFBundleDisplayName</key><string>Ferd</string>
    <key>CFBundleIdentifier</key><string>io.github.scorphus.ferd</string>
    <key>CFBundleExecutable</key><string>Ferd</string>
    <key>CFBundleIconFile</key><string>Ferd</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Schema</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>Alternate</string>
            <key>CFBundleTypeExtensions</key>
            <array><string>md</string><string>markdown</string><string>sql</string></array>
        </dict>
    </array>
</dict>
</plist>
PLIST
codesign --force -s - "$app"
echo "$app"
