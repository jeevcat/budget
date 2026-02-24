#!/bin/bash
# Async: run clippy and tests after Rust file edits

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

if [[ "$FILE_PATH" != *.rs ]]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"

CLIPPY_RESULT=$(cargo clippy --all-targets -- -D warnings 2>&1)
CLIPPY_EXIT=$?

if [ $CLIPPY_EXIT -ne 0 ]; then
  echo "{\"systemMessage\": \"Clippy failed after editing $FILE_PATH:\\n$CLIPPY_RESULT\"}"
  exit 0
fi

TEST_RESULT=$(cargo test 2>&1)
TEST_EXIT=$?

if [ $TEST_EXIT -eq 0 ]; then
  echo "{\"systemMessage\": \"Clippy and tests passed after editing $FILE_PATH\"}"
else
  echo "{\"systemMessage\": \"Tests failed after editing $FILE_PATH:\\n$TEST_RESULT\"}"
fi
