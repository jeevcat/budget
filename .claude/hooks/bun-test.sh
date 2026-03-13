#!/usr/bin/env bash
# Async: run frontend tests after JS edits

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

case "$FILE_PATH" in
  *.js) ;;
  *) exit 0 ;;
esac

cd "$CLAUDE_PROJECT_DIR"

TEST_RESULT=$(bun test frontend/ 2>&1)
TEST_EXIT=$?

if [ $TEST_EXIT -ne 0 ]; then
  echo "{\"systemMessage\": \"Frontend tests failed after editing $FILE_PATH:\\n$TEST_RESULT\"}"
else
  echo "{\"systemMessage\": \"Frontend tests passed after editing $FILE_PATH\"}"
fi
