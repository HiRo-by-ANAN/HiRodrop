#!/bin/zsh
set -euo pipefail

OUTPUT_PATH="${1:?usage: generate-third-party-notices.sh OUTPUT_PATH}"
PROJECT_DIR="${0:A:h:h:h}"
TREE_FILE="$(mktemp)"
trap 'rm -f "$TREE_FILE"' EXIT

cd "$PROJECT_DIR"
cargo tree --locked \
    --target aarch64-apple-darwin \
    --edges normal,build \
    --prefix none \
    --format '{p}' | sort -u > "$TREE_FILE"

{
    print "HiRodrop third-party notices"
    print "Generated from the Cargo.lock dependency tree for macOS."
    print "Each package remains subject to its own terms below."
} > "$OUTPUT_PATH"

while read -r PACKAGE_NAME PACKAGE_VERSION REST; do
    if [[ "$PACKAGE_NAME" == "hirodrop-core" ]]; then
        continue
    fi
    PACKAGE_VERSION="${PACKAGE_VERSION#v}"
    PACKAGE_DIR="$(find "$HOME/.cargo/registry/src" \
        -maxdepth 2 \
        -type d \
        -name "$PACKAGE_NAME-$PACKAGE_VERSION" \
        -print \
        -quit 2>/dev/null)"
    if [[ -z "$PACKAGE_DIR" ]]; then
        print -u2 "Missing Cargo source for $PACKAGE_NAME $PACKAGE_VERSION"
        exit 1
    fi
    MANIFEST="$PACKAGE_DIR/Cargo.toml"
    LICENSE_EXPRESSION="$(sed -n 's/^license = "\([^"]*\)"/\1/p' "$MANIFEST" | head -1)"
    REPOSITORY="$(sed -n 's/^repository = "\([^"]*\)"/\1/p' "$MANIFEST" | head -1)"
    {
        print ""
        print "================================================================"
        print "$PACKAGE_NAME $PACKAGE_VERSION"
        print "Declared license: ${LICENSE_EXPRESSION:-see included license file}"
        [[ -n "$REPOSITORY" ]] && print "Source: $REPOSITORY"
        print "================================================================"
    } >> "$OUTPUT_PATH"

    LICENSE_FILES=("$PACKAGE_DIR"/(LICENSE*|COPYING*|NOTICE*)(N-.))
    if (( ${#LICENSE_FILES[@]} == 0 )); then
        print "No top-level license text was included in the downloaded crate." >> "$OUTPUT_PATH"
        continue
    fi
    for LICENSE_FILE in "${LICENSE_FILES[@]}"; do
        print "" >> "$OUTPUT_PATH"
        print -r -- "--- ${LICENSE_FILE:t} ---" >> "$OUTPUT_PATH"
        cat "$LICENSE_FILE" >> "$OUTPUT_PATH"
        print "" >> "$OUTPUT_PATH"
    done
done < "$TREE_FILE"
