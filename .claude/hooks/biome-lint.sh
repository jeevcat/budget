#!/usr/bin/env bash
# Async: run Biome lint after JS/CSS/HTML edits

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

case "$FILE_PATH" in
  *.js|*.css|*.html) ;;
  *) exit 0 ;;
esac

cd "$CLAUDE_PROJECT_DIR"

LINT_RESULT=$(biome lint "$FILE_PATH" 2>&1)
LINT_EXIT=$?

if [ $LINT_EXIT -ne 0 ]; then
  echo "{\"systemMessage\": \"Biome lint failed for $FILE_PATH:\\n$LINT_RESULT\"}"
else
  echo "{\"systemMessage\": \"Biome lint passed for $FILE_PATH\"}"
fi
