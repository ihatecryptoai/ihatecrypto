#!/bin/zsh
# Checks that a published release is this code: builds the release's tag the way release.sh did, takes the code
# signatures off both copies (only the project's keys can make those) and compares every file.
#   ./verify-release.sh 0.7.0                     the DMG from the GitHub release
#   ./verify-release.sh 0.7.0 <.dmg|.tar.xz|.app>  a copy you have (the update archive from updates.ihatecrypto.ai, an
#                                                  installed app)
# Needs the tag (git fetch --tags) and the Xcode the release was built with (another Xcode in /Applications is picked
# by itself). XcodeGen is downloaded if yours isn't the release's. Takes about 5 minutes; nothing is installed or run.
set -e
version=${1#v}; given=${2:+${2:A}}
[[ -n $version ]] || { echo "usage: ./verify-release.sh <version> [<.dmg|.tar.xz|.app>]"; exit 1; }
cd "$(dirname "$0")"
root=/private/tmp/ihatecrypto-release
git rev-parse -q --verify "v$version^{commit}" > /dev/null || { echo "no tag v$version here: git fetch --tags"; exit 1; }
pgrep -qf "derivedDataPath $root/build" && { echo "a release build is running in $root: try again after it"; exit 1; }
rm -rf $root; mkdir -p $root/src $root/release

echo "· the release"
file=${given:-$root/ihatecrypto-$version.dmg}
[[ -n $given ]] || curl -fsSL -o $file "https://github.com/ihatecryptoai/ihatecrypto/releases/download/v$version/ihatecrypto-$version.dmg"
case $file in
  *.dmg) mnt=$(mktemp -d); hdiutil attach -readonly -nobrowse -noautoopen -mountpoint $mnt "$file" > /dev/null
         ditto $mnt/ihatecrypto.app $root/release/ihatecrypto.app; hdiutil detach $mnt -quiet ;;
  *.tar.xz) tar -xJf "$file" -C $root/release ;;
  *.app) ditto "$file" $root/release/ihatecrypto.app ;;
  *) echo "a .dmg, .tar.xz or .app, please"; exit 1 ;;
esac
plist=$root/release/ihatecrypto.app/Contents/Info.plist
key() { /usr/libexec/PlistBuddy -c "Print :$1" $plist 2> /dev/null || true; }
[[ $(key CFBundleShortVersionString) == $version ]] || { echo "that app is version $(key CFBundleShortVersionString), not $version"; exit 1; }
(cd .. && git archive "v$version") | tar -x -C $root/src
[[ -x $root/src/App/Tools/build-release.sh && -n $(key XcodeGenVersion) ]] \
  || { echo "$version was released before builds could be checked: that starts with the version after 0.6.9"; exit 1; }

# The tools the release names: Xcode by its build, XcodeGen by version.
xcode=$(key DTXcodeBuild)
if [[ $(xcodebuild -version | awk '/Build version/ { print $3 }') != $xcode ]]; then
  for x in /Applications/Xcode*.app; do
    if [[ $(/usr/libexec/PlistBuddy -c "Print :ProductBuildVersion" $x/Contents/version.plist 2> /dev/null) == $xcode ]]; then
      export DEVELOPER_DIR=$x/Contents/Developer; break
    fi
  done
  [[ -n $DEVELOPER_DIR ]] || { echo "the release was built with Xcode build $xcode: install that Xcode (xcodes.app or developer.apple.com)"; exit 1; }
fi
xcodegen=$(key XcodeGenVersion)
if [[ $(xcodegen --version 2> /dev/null) != "Version: $xcodegen" ]]; then
  curl -fsSL -o $root/xcodegen.zip "https://github.com/yonaskolb/XcodeGen/releases/download/$xcodegen/xcodegen.zip"
  ditto -x -k $root/xcodegen.zip $root/tools
  export PATH=$root/tools/xcodegen/bin:$PATH
fi

echo "· building v$version (build $(key CFBundleVersion)) with Xcode $xcode and XcodeGen $xcodegen, unsigned"
$root/src/App/Tools/build-release.sh $version $(key CFBundleVersion) "$(key SUFeedURL)" $root/built.xcarchive --unsigned

echo "· comparing"
# Each Mach-O file loses its signature; the signatures' own files go: _CodeSignature, the stapled notarization ticket
# (Contents/CodeResources) and the signer's provisioning profile.
unsign() {
  ditto "$1" "$2"
  find "$2" \( -name _CodeSignature -o -name embedded.provisionprofile \) -prune -exec rm -rf {} +
  find "$2" -path '*/Contents/CodeResources' -delete
  # codesign --remove-signature leaves __LINKEDIT's size in memory as big as the signature made it, so on both copies
  # it is set from the segment's size in the file, rounded to 16 KB pages, as the linker sets it for an unsigned file.
  python3 -I - "$2" <<'EOF'
import os, struct, subprocess, sys
for top, _, names in os.walk(sys.argv[1]):
    for name in names:
        path = os.path.join(top, name)
        if os.path.islink(path): continue
        with open(path, 'rb') as f: magic = f.read(4)
        if magic not in (b'\xca\xfe\xba\xbe', b'\xcf\xfa\xed\xfe'): continue
        subprocess.run(['codesign', '--remove-signature', path], check=True)
        data = bytearray(open(path, 'rb').read())
        fat = data[:4] == b'\xca\xfe\xba\xbe'
        slices = [struct.unpack_from('>I', data, 16 + 20 * i)[0] for i in range(struct.unpack_from('>I', data, 4)[0])] if fat else [0]
        for base in slices:
            commands, at = struct.unpack_from('<I', data, base + 16)[0], base + 32
            for _ in range(commands):
                command, size = struct.unpack_from('<II', data, at)
                if command == 0x19 and data[at + 8:at + 24].rstrip(b'\0') == b'__LINKEDIT':
                    struct.pack_into('<Q', data, at + 32, (struct.unpack_from('<Q', data, at + 48)[0] + 0x3fff) & ~0x3fff)
                at += size
        open(path, 'wb').write(data)
EOF
}
list() {
  (cd "$1" && find . | LC_ALL=C sort | while IFS= read -r p; do
    if [[ -L $p ]]; then echo "link $p -> $(readlink "$p")"
    elif [[ -d $p ]]; then echo "dir  $p"
    else echo "file $p $(shasum -a 256 "$p" | cut -c1-64)$([[ -x $p ]] && echo ' executable')"; fi
  done)
}
unsign $root/release/ihatecrypto.app $root/compare/release.app
unsign $root/built.xcarchive/Products/Applications/ihatecrypto.app $root/compare/built.app
# Xcode writes the build Mac's macOS into Info.plist, and a build phase deletes it (project.yml). Before 0.7.2 that phase
# could run before Xcode had written the file, and on GitHub's Mac the key stayed. The same deletion here, only where
# the release has no such key: the build machine's macOS isn't code.
machine() { /usr/libexec/PlistBuddy -c "Print :BuildMachineOSBuild" "$1" > /dev/null 2>&1; }
for p in Contents/Info.plist Contents/XPCServices/Signer.xpc/Contents/Info.plist; do
  if ! machine $root/compare/release.app/$p && machine $root/compare/built.app/$p; then
    /usr/libexec/PlistBuddy -c "Delete :BuildMachineOSBuild" $root/compare/built.app/$p
  fi
done
list $root/compare/release.app > $root/compare/release.txt
list $root/compare/built.app > $root/compare/built.txt
if diff $root/compare/release.txt $root/compare/built.txt > $root/compare/diff.txt; then
  echo "✓ ihatecrypto $version is the code at v$version ($(git rev-parse --short "v$version^{commit}")):" \
       "$(grep -c '^file' $root/compare/built.txt) files identical apart from code signatures"
  rm -rf $root
else
  echo "✗ ihatecrypto $version differs from the code at v$version (< release, > built from the code):"
  cat $root/compare/diff.txt
  # A property list that differs: its keys side by side, so a run on another Mac says why without the files.
  for p in $(grep -E '^< file .*\.plist ' $root/compare/diff.txt | awk '{ print $3 }'); do
    echo "· $p (< release, > built):"
    diff <(plutil -p $root/compare/release.app/$p) <(plutil -p $root/compare/built.app/$p) || true
  done
  echo "both copies, signatures off: $root/compare"
  exit 1
fi
