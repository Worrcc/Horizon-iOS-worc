#!/usr/bin/env bash
# Type-checks the game for iOS (aarch64-apple-ios) on any host, Linux included.
# `cargo check` never links, so a few stub headers stand in for the iOS SDK where
# C build scripts (ring) want one. On a Mac with Xcode the real SDK is used.
#
#   ios/scripts/check-ios.sh [extra cargo args]
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
target="${HORIZON_IOS_TARGET:-aarch64-apple-ios}"
rustup target add "$target" >/dev/null
if ! command -v xcrun >/dev/null 2>&1; then
    stub="${CARGO_TARGET_DIR:-$root/target}/ios-stub-sdk"
    mkdir -p "$stub/usr/include"
    cat > "$stub/usr/include/TargetConditionals.h" <<'H'
#pragma once
#define TARGET_OS_MAC 1
#define TARGET_OS_IPHONE 1
#define TARGET_OS_IOS 1
#define TARGET_OS_OSX 0
#define TARGET_OS_SIMULATOR 0
#define TARGET_OS_WATCH 0
#define TARGET_OS_TV 0
#define TARGET_OS_VISION 0
#define TARGET_OS_MACCATALYST 0
#define TARGET_OS_EMBEDDED 1
H
    printf '#pragma once\n#define assert(x) ((void)0)\n' > "$stub/usr/include/assert.h"
    # wasmtime's C helpers (the `mods` feature).
    printf '#pragma once\n#include <stddef.h>\nvoid abort(void);\n' > "$stub/usr/include/stdlib.h"
    printf '#pragma once\n#include <stddef.h>\nvoid *memcpy(void*,const void*,size_t);\nvoid *memset(void*,int,size_t);\nint memcmp(const void*,const void*,size_t);\nvoid *memmove(void*,const void*,size_t);\nsize_t strlen(const char*);\n' > "$stub/usr/include/string.h"
    export SDKROOT="$stub"
    triple="${target//-/_}"
    export "CC_${triple}=clang"
    export "CFLAGS_${triple}=--target=arm64-apple-ios15.0 -ffreestanding"
fi
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}"
cd "$root"
exec cargo check --target "$target" -p horizon-client --bins --lib --features mods "$@"
