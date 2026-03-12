#!/usr/bin/env bash
# Format Kotlin files after edits

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

case "$FILE_PATH" in
  *.kt|*.kts) ;;
  *) exit 0 ;;
esac

cd "$CLAUDE_PROJECT_DIR/mobile"
./gradlew spotlessApply --quiet 2>/dev/null
