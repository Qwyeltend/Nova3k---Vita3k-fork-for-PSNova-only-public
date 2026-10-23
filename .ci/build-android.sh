#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "$0")/common.sh"

OUTPUT_DIR="${1:-}"
if [[ -z "$OUTPUT_DIR" ]]; then
    echo "Usage: $0 <output-dir>" >&2
    exit 1
fi

# Nova3K: apply our patches to third-party submodules (idempotent)
# tools/nova3k/patches/<path under external/>/*.patch, e.g. psvpfstools/psvpfsparser for a nested submodule
while IFS= read -r patch; do
    [[ -e "$patch" ]] || continue
    rel="${patch#tools/nova3k/patches/}"
    module="external/$(dirname "$rel")"
    if git -C "$module" apply --check "$PWD/$patch" 2>/dev/null; then
        git -C "$module" apply "$PWD/$patch"
        echo "Nova3K: applied $patch to $module"
    elif git -C "$module" apply --reverse --check "$PWD/$patch" 2>/dev/null; then
        echo "Nova3K: $patch already applied to $module"
    else
        echo "Nova3K: $patch does not apply to $module" >&2
        exit 1
    fi
done < <(find tools/nova3k/patches -name '*.patch' | sort)

mkdir -p android/app/assets
rm -rf android/app/assets/data android/app/assets/shaders-builtin
cp -r data android/app/assets/data
cp -r vita3k/shaders-builtin android/app/assets/shaders-builtin

download_android_strings android/app/src/main/res

# Nova3K: show our own name in every language
find android/app/src/main/res -name strings.xml -path '*values*' \
    -exec sed -i 's|<string name="app_name">[^<]*</string>|<string name="app_name">Nova3K</string>|' {} +

chmod +x android/gradlew

# Nova3K: always an optimized release build. It is signed with the fixed
# key from android/nova3k.jks (set by the workflow), so every new APK
# installs over the previous one without losing data.
BUILD_TYPE="Release"

pushd android > /dev/null
./gradlew --stacktrace ":app:assemble${BUILD_TYPE}"
popd > /dev/null

APK_PATH="android/app/build/outputs/apk/${BUILD_TYPE,,}/app-${BUILD_TYPE,,}.apk"
mkdir -p "$OUTPUT_DIR"
cp "$APK_PATH" "$OUTPUT_DIR/app.apk"
