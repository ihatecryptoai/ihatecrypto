#!/bin/zsh
# The release build, shared by release.sh (signed) and verify-release.sh (--unsigned), so a release can be rebuilt
# byte for byte apart from its code signatures. It runs in a copy of the committed tree at /private/tmp/ihatecrypto-release/src,
# which those two scripts make: the folder's path goes into the binaries (their UUIDs), so it's the same on every Mac.
#   Tools/build-release.sh <version> <build> <feed url or ""> <archive path> [--unsigned]
set -e
version=$1 build=$2 feed=$3 archive=$4 mode=$5
root=/private/tmp/ihatecrypto-release
[[ ${0:A:h:h} == $root/src/App ]] || { echo "run from $root/src/App: release.sh and verify-release.sh copy the tree there"; exit 1; }
cd $root/src/App
# Only an XcodeGen whose version has a checksum in Tools/xcodegen.sha256: whoever checks this release must be able to
# get that version and know its bytes (the 2026-11-13 review). The xcodegen on this Mac isn't hashed (Homebrew's build
# isn't GitHub's zip), only its version is held to the table: a rebuild with another copy is what would show a bad one.
xcodegen=$(xcodegen --version | awk '{ print $2 }')
[[ -n $(awk -v v="$xcodegen" '$1 "" == v "" { print $2 }' Tools/xcodegen.sha256) ]] \
  || { echo "XcodeGen «$xcodegen» has no checksum in Tools/xcodegen.sha256, so nobody could check this release: add it (the file says how) and commit"; exit 1; }
(cd Xcode && xcodegen generate --quiet)
# Private Cloud Compute is Apple's for Development, Ad hoc and App Store profiles only: the Developer ID copy goes without
# it (its Apple models stay the one on the Mac). Absent until the capability is on the App ID app.ihatecrypto.mac.
pcc=":com.apple.developer.private-cloud-compute"
/usr/libexec/PlistBuddy -c "Print $pcc" Xcode/ihatecrypto.entitlements > /dev/null 2>&1 && /usr/libexec/PlistBuddy -c "Delete $pcc" Xcode/ihatecrypto.entitlements
# Unsigned, the linker would sign the arm64 binaries ad hoc, and the room for that signature moves the code by 16 bytes.
signing=(-allowProvisioningUpdates)
[[ $mode == --unsigned ]] && signing=(CODE_SIGNING_ALLOWED=NO 'OTHER_LDFLAGS=$(inherited) -Xlinker -no_adhoc_codesign')
xcodebuild -project Xcode/ihatecrypto.xcodeproj -scheme ihatecrypto -configuration Release -derivedDataPath $root/build \
  -archivePath "$archive" "${signing[@]}" \
  MARKETING_VERSION=$version CURRENT_PROJECT_VERSION=$build SPARKLE_FEED_URL=$feed \
  XCODEGEN_VERSION=$xcodegen archive > $root/build.log 2>&1 || {
  grep -E "error:|Command .* failed|FAILED" $root/build.log | cut -c1-300 | head -20; echo "full log: $root/build.log"; exit 1; }
