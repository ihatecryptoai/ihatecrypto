#!/bin/zsh
# An update for the site: ./release.sh 0.2 https://example.com/ihatecrypto
#   → dist/site/ihatecrypto-0.2.tar.xz and dist/site/appcast.xml (with the release notes inside), plus dist/ihatecrypto-0.2.dmg
#     for a first install. Upload dist/site/ to that URL: copies installed from that disk image find the appcast there and
#     update themselves. Keep dist/site/ between releases: the old archives make the small .delta updates.
# A copy to hand over: ./release.sh 0.2   → dist/ihatecrypto-0.2.dmg, notarized, no update checks. Opens on any Mac
#   with macOS 26 by a double click and starts on the first screen; the Keychain and Application Support stay here.
# Needs, once on this Mac:
#   - the «Developer ID Application» certificate of team N78L833HCS in the login Keychain (Xcode › Settings › Accounts);
#   - a notarytool profile: xcrun notarytool store-credentials notary --apple-id … --team-id N78L833HCS
#   - Sparkle's private key, in the login Keychain only (generate_keys made it; its public half is in
#     Xcode/ihatecrypto-Info.plist). Never export it to a file. If it's lost, it can be replaced: releases carry the team's
#     Developer ID signature, and Sparkle takes an update that changes the EdDSA key as long as that signature stays the
#     team's — a new key (generate_keys) and its SUPublicEDKey in the next release. Never change both at once.
# Team and bundle id stay N78L833HCS / app.ihatecrypto.mac, or the signer loses its Keychain items.
# Built from the committed tree (HEAD) by Tools/build-release.sh, the same build verify-release.sh makes, so anyone can
# check a release against the code: commit first, the script refuses uncommitted changes to Sources.
set -e
version=$1; site=${2%/}
[[ -n $version && ( -z $site || $site == https://* ) ]] || { echo "usage: ./release.sh <version> [<https://site/path>]"; exit 1; }
cd "$(dirname "$0")"
git diff --quiet HEAD -- Sources Xcode || { echo "Sources or Xcode have uncommitted changes: commit them first."; exit 1; }
python3 Tools/release-notes.py "$version" > /dev/null || { echo "Sources/IhatecryptoApp/WhatsNew/whatsnew.json has no entry for $version."; exit 1; }
# The build number counts commits, so every release is newer than the last. Opening the code (2026-10-21) took 32
# commits out of the history: 0.6.9 was build 580 at what is now commit 548.
build=$(( $(git rev-list --count HEAD) + 32 ))
out=dist; root=/private/tmp/ihatecrypto-release
sparkle=$root/build/SourcePackages/artifacts/sparkle/Sparkle/bin
mkdir -p $out
log=$(mktemp -t ihatecrypto-release)
step() { echo "· $1"; }
fail() { grep -E "error:|Command .* failed|FAILED" "$log" | cut -c1-300 | head -20; echo "full log: $log"; exit 1; }

step "archive $version ($build)"
pgrep -qf "derivedDataPath $root/build" && { echo "another release build is using $root"; exit 1; }
rm -rf $root; mkdir -p $root/src
(cd .. && git archive HEAD) | tar -x -C $root/src
$root/src/App/Tools/build-release.sh $version $build "${site:+$site/appcast.xml}" $PWD/$out/ihatecrypto.xcarchive

step "sign with Developer ID"
cat > $out/export.plist <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>method</key><string>developer-id</string>
  <key>teamID</key><string>N78L833HCS</string>
  <key>signingStyle</key><string>automatic</string>
</dict></plist>
EOF
rm -rf $out/export
xcodebuild -exportArchive -archivePath $out/ihatecrypto.xcarchive -exportPath $out/export -exportOptionsPlist $out/export.plist \
  -allowProvisioningUpdates > "$log" 2>&1 || fail
app=$out/export/ihatecrypto.app
codesign --verify --deep --strict "$app"
# The working copy's identity and nothing else (build-app.sh's development copy has its own): with another bundle id or
# Keychain group the update's signer would not find the wallets.
id() { /usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$1/Contents/Info.plist"; }
[[ $(id "$app") == app.ihatecrypto.mac && $(id "$app/Contents/XPCServices/Signer.xpc") == app.ihatecrypto.mac.signer ]] \
  && codesign -d --entitlements - --xml "$app/Contents/XPCServices/Signer.xpc" 2>/dev/null | grep -q ">N78L833HCS.app.ihatecrypto.mac.signer<" \
  || { echo "not the working copy's bundle ids or Keychain group: nothing is published."; exit 1; }
[[ -z $site || $(/usr/libexec/PlistBuddy -c "Print :SUFeedURL" "$app/Contents/Info.plist") == $site/appcast.xml ]] || { echo "the feed didn't make it into Info.plist"; exit 1; }

step "notarize (a few minutes)"
ditto -c -k --keepParent "$app" $out/notarize.zip
xcrun notarytool submit $out/notarize.zip --keychain-profile notary --wait
rm $out/notarize.zip
xcrun stapler staple "$app"
spctl --assess --type execute "$app"

step "disk image"
dmg=$out/ihatecrypto-$version.dmg
rm -rf $out/dmg $out/rw.dmg $dmg; mkdir -p $out/dmg/.background
ditto "$app" $out/dmg/ihatecrypto.app
ln -s /Applications $out/dmg/Applications
cp Tools/dmg/background.tiff $out/dmg/.background/
cp "$app/Contents/Resources/AppIcon.icns" $out/dmg/.VolumeIcon.icns
# Read-write first: Finder lays out the window (the background with its arrow, the app left, Applications right;
# Tools/dmg), then it's compressed. macOS may ask once to let this control Finder; without it the window stays plain.
hdiutil create -volname ihatecrypto -srcfolder $out/dmg -fs HFS+ -format UDRW $out/rw.dmg > /dev/null
mount=$(hdiutil attach -readwrite -noverify -noautoopen $out/rw.dmg | awk -F'\t' '/\/Volumes\// { print $NF }')
xcrun SetFile -a C "$mount"
osascript Tools/dmg/layout.applescript "${mount#/Volumes/}" || echo "  Finder didn't lay out the window: the disk image works, without its background."
sync
hdiutil detach "$mount" -quiet
# LZMA (ULMO): the zlib image outgrew the host's 25 MiB per file at 0.6.7 (26.6 MB; LZMA 21.8 MB).
hdiutil convert $out/rw.dmg -format ULMO -o $dmg > /dev/null
rm -f $out/rw.dmg
identity=$(security find-identity -v -p codesigning | awk -F'"' '/Developer ID Application.*N78L833HCS/ { print $2; exit }')
[[ -n $identity ]] && codesign --sign "$identity" --timestamp $dmg
xcrun notarytool submit $dmg --keychain-profile notary --wait
xcrun stapler staple $dmg
rm -rf $out/dmg
if [[ -z $site ]]; then
  rm -rf $out/export $out/ihatecrypto.xcarchive $out/export.plist $root
  echo "done: $out/ihatecrypto-$version.dmg — hand over this file only"
  exit 0
fi

step "archive and appcast"
# Their own folder: generate_appcast reads every archive beside the feed, and the disk images in dist/ aren't updates.
mkdir -p $out/site
# tar.xz, not zip: the zip outgrew the host's 25 MiB per file at 0.6.7 (26.8 MB; xz 16 MB). Sparkle 2 unpacks it; no
# AppleDouble files or xattrs go in, the signature and the stapled ticket live in the bundle's own files.
(cd "${app:h}" && COPYFILE_DISABLE=1 tar --no-mac-metadata --no-xattrs --options xz:compression-level=9 -cJf "$OLDPWD/$out/site/ihatecrypto-$version.tar.xz" "${app:t}")
python3 Tools/release-notes.py "$version" > $out/site/ihatecrypto-$version.html
rm -rf $out/export $out/ihatecrypto.xcarchive $out/export.plist
# Signs the new archive (and the feed: the app requires a signed one) with the key in the Keychain; the notes go inside.
# The landing's disk image would read as one more archive of the same version, so it steps out while the feed is made.
rm -f $out/site/ihatecrypto.dmg
$sparkle/generate_appcast --download-url-prefix "$site/" $out/site
# The landing (Tools/site) shares the folder: ihatecrypto.ai downloads this release's disk image.
# The page and its manifesto in English, Spanish and Russian (/, /es/, /ru/ and manifesto/ under each), each in its own words for search engines.
python3 Tools/site/build.py "$version" $out/site >/dev/null
cp Tools/site/{_headers,icon.png,favicon.png,og.png,llms.txt,robots.txt,sitemap.xml,three.min.js} $out/site/
cp -R Tools/site/{fonts,mf} $out/site/
cp $dmg $out/site/ihatecrypto.dmg
rm -rf "$log" $root
echo "done: publish $out/site/ to $site/ (updates.ihatecrypto.ai and the landing ihatecrypto.ai: npx wrangler deploy -c Tools/updates/wrangler.jsonc); $out/ihatecrypto-$version.dmg is the first install (the landing's download, or a friend)"
