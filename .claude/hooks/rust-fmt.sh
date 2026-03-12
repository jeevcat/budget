#!/usr/bin/env bash
# Format Rust files after edits

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

if [[ "$FILE_PATH" != *.rs ]]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"
cargo fmt --quiet 2>/dev/null
