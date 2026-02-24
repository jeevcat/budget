#!/bin/bash
# Format JS/CSS files after edits

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

case "$FILE_PATH" in
  *.js|*.css|*.html) ;;
  *) exit 0 ;;
esac

cd "$CLAUDE_PROJECT_DIR"
biome format --write "$FILE_PATH" 2>/dev/null
