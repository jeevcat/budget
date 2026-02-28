#!/bin/bash
# Async: run detekt after Kotlin edits

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

case "$FILE_PATH" in
  *.kt|*.kts) ;;
  *) exit 0 ;;
esac

cd "$CLAUDE_PROJECT_DIR/mobile"

LINT_RESULT=$(./gradlew detekt --console=plain --no-problems-report 2>&1)
LINT_EXIT=$?

if [ $LINT_EXIT -ne 0 ]; then
  echo "{\"systemMessage\": \"detekt failed:\\n$LINT_RESULT\"}"
else
  echo "{\"systemMessage\": \"detekt passed\"}"
fi
