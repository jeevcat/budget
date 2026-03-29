# iOS CI/CD and Tooling Configuration

This document contains all GitHub Actions, Git hooks, and Claude hooks for the iOS app.

## GitHub Actions

### `.github/workflows/ios.yml`

```yaml
name: iOS

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

# Only one iOS build at a time
concurrency:
  group: ios-${{ github.ref }}
  cancel-in-progress: true

jobs:
  ios:
    name: iOS Build and Test
    runs-on: macos-15  # iOS 26 SDK requires macOS 15+ (Sequoia)
    defaults:
      run:
        working-directory: ios
    steps:
      - uses: actions/checkout@v6

      - name: Select Xcode 16
        run: sudo xcode-select -s /Applications/Xcode_16.app

      - name: Install dependencies
        run: |
          brew install swiftformat
          brew install swiftlint
          brew install xcbeautify

      - name: SwiftFormat check
        run: swiftformat --lint --swiftversion 6 .

      - name: SwiftLint
        run: swiftlint lint --strict

      - name: Build
        run: |
          xcodebuild clean build \
            -scheme BudgetApp \
            -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=26.0' \
            -configuration Debug \
            CODE_SIGNING_ALLOWED=NO \
            | xcbeautify

      - name: Run tests
        run: |
          xcodebuild test \
            -scheme BudgetApp \
            -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=26.0' \
            -configuration Debug \
            CODE_SIGNING_ALLOWED=NO \
            -enableCodeCoverage YES \
            | xcbeautify

      - name: Archive coverage
        uses: actions/upload-artifact@v7
        if: success()
        with:
          name: ios-code-coverage
          path: ios/build/CodeCoverage
          retention-days: 30

  build-release:
    name: Build iOS IPA
    runs-on: macos-15
    if: github.ref == 'refs/heads/main'
    needs: ios
    permissions:
      contents: write
    defaults:
      run:
        working-directory: ios
    steps:
      - uses: actions/checkout@v6

      - name: Select Xcode 16
        run: sudo xcode-select -s /Applications/Xcode_16.app

      - name: Get version
        id: version
        run: echo "version=$(date +'%Y.%m.%d')-${GITHUB_SHA::7}" >> "$GITHUB_OUTPUT"

      - name: Install provisioning profile
        env:
          PROVISIONING_PROFILE_BASE64: ${{ secrets.IOS_PROVISIONING_PROFILE_BASE64 }}
        run: |
          mkdir -p ~/Library/MobileDevice/Provisioning\ Profiles
          echo "$PROVISIONING_PROFILE_BASE64" | base64 -d > \
            ~/Library/MobileDevice/Provisioning\ Profiles/budget.mobileprovision

      - name: Install certificate
        env:
          CERTIFICATE_BASE64: ${{ secrets.IOS_CERTIFICATE_BASE64 }}
          CERTIFICATE_PASSWORD: ${{ secrets.IOS_CERTIFICATE_PASSWORD }}
        run: |
          echo "$CERTIFICATE_BASE64" | base64 -d > certificate.p12
          security create-keychain -p "" build.keychain
          security default-keychain -s build.keychain
          security unlock-keychain -p "" build.keychain
          security import certificate.p12 -k build.keychain \
            -P "$CERTIFICATE_PASSWORD" -T /usr/bin/codesign
          security set-key-partition-list -S apple-tool:,apple: -s -k "" build.keychain
          rm certificate.p12

      - name: Build archive
        run: |
          xcodebuild archive \
            -scheme BudgetApp \
            -configuration Release \
            -archivePath build/BudgetApp.xcarchive \
            -destination 'generic/platform=iOS' \
            MARKETING_VERSION=${{ steps.version.outputs.version }} \
            CURRENT_PROJECT_VERSION=${{ github.run_number }}

      - name: Export IPA
        run: |
          xcodebuild -exportArchive \
            -archivePath build/BudgetApp.xcarchive \
            -exportPath build \
            -exportOptionsPlist ExportOptions.plist

      - name: Create release
        uses: softprops/action-gh-release@v2
        with:
          tag_name: ios-v${{ steps.version.outputs.version }}
          name: iOS ${{ steps.version.outputs.version }}
          files: ios/build/BudgetApp.ipa
          prerelease: true
          generate_release_notes: true
```

## Git Pre-Commit Hook

### Update to `.github/hooks/pre-commit` (replace mobile/Android section)

```bash
# Replace lines 77-190 (mobile section) with iOS checks:

if git diff --cached --name-only | grep -q '^ios/'; then
    echo -n "Checking Swift formatting... "
    if ! command -v swiftformat &> /dev/null; then
        echo -e "${RED}SKIPPED${NC} (swiftformat not installed: brew install swiftformat)"
    else
        output=$(swiftformat --lint --swiftversion 6 ios/ 2>&1) || {
            echo -e "${RED}FAILED${NC}"
            echo "$output"
            echo ""
            echo "To fix, run:"
            echo "  swiftformat --swiftversion 6 ios/"
            exit 1
        }
        echo -e "${GREEN}OK${NC}"
    fi

    echo -n "Running SwiftLint... "
    if ! command -v swiftlint &> /dev/null; then
        echo -e "${RED}SKIPPED${NC} (swiftlint not installed: brew install swiftlint)"
    else
        output=$(cd ios && swiftlint lint --strict 2>&1) || {
            echo -e "${RED}FAILED${NC}"
            echo "$output"
            echo ""
            echo "To fix auto-fixable issues, run:"
            echo "  cd ios && swiftlint --fix"
            exit 1
        }
        echo -e "${GREEN}OK${NC}"
    fi

    echo -n "Compiling Swift... "
    if ! command -v xcodebuild &> /dev/null; then
        echo -e "${RED}SKIPPED${NC} (Xcode not installed)"
    else
        output=$(xcodebuild build \
            -scheme BudgetApp \
            -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=26.0' \
            -configuration Debug \
            CODE_SIGNING_ALLOWED=NO \
            -quiet 2>&1) || {
            echo -e "${RED}FAILED${NC}"
            echo "$output"
            echo ""
            echo "To see errors again, run:"
            echo "  cd ios && xcodebuild build -scheme BudgetApp -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=26.0'"
            exit 1
        }
        echo -e "${GREEN}OK${NC}"
    fi

    echo -n "Running iOS unit tests... "
    if ! command -v xcodebuild &> /dev/null; then
        echo -e "${RED}SKIPPED${NC} (Xcode not installed)"
    else
        output=$(xcodebuild test \
            -scheme BudgetApp \
            -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=26.0' \
            -configuration Debug \
            CODE_SIGNING_ALLOWED=NO \
            -quiet 2>&1) || {
            echo -e "${RED}FAILED${NC}"
            echo "$output"
            echo ""
            echo "To see failures, run:"
            echo "  cd ios && xcodebuild test -scheme BudgetApp -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=26.0'"
            exit 1
        }
        echo -e "${GREEN}OK${NC}"
    fi
fi
```

## Claude Hooks

### `.claude/hooks/swift-fmt.sh`

```bash
#!/usr/bin/env bash
# Auto-format Swift files after Edit/Write

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

case "$FILE_PATH" in
  *.swift) ;;
  *) exit 0 ;;
esac

if ! command -v swiftformat &> /dev/null; then
    exit 0
fi

cd "$CLAUDE_PROJECT_DIR/ios"
swiftformat --swiftversion 6 "$FILE_PATH" 2>/dev/null
```

### `.claude/hooks/swift-lint.sh`

```bash
#!/usr/bin/env bash
# Run SwiftLint after Edit/Write (async)

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

case "$FILE_PATH" in
  *.swift) ;;
  *) exit 0 ;;
esac

if ! command -v swiftlint &> /dev/null; then
    exit 0
fi

cd "$CLAUDE_PROJECT_DIR/ios"
swiftlint lint --path "$FILE_PATH" --quiet 2>&1
```

### `.claude/hooks/swift-build.sh`

```bash
#!/usr/bin/env bash
# Build iOS app after Edit/Write to catch compile errors (async)

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

case "$FILE_PATH" in
  *.swift) ;;
  *) exit 0 ;;
esac

if ! command -v xcodebuild &> /dev/null; then
    exit 0
fi

cd "$CLAUDE_PROJECT_DIR/ios"
xcodebuild build \
    -scheme BudgetApp \
    -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=26.0' \
    -configuration Debug \
    CODE_SIGNING_ALLOWED=NO \
    -quiet 2>&1 | grep -E 'error:|warning:' || true
```

### Update `.claude/settings.json` (replace Kotlin hooks with Swift hooks)

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/scripts/setup-sandbox.sh",
            "timeout": 60
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/rust-fmt.sh",
            "timeout": 10
          }
        ]
      },
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/rust-test.sh",
            "timeout": 300,
            "async": true
          }
        ]
      },
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/biome-fmt.sh",
            "timeout": 10
          }
        ]
      },
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/biome-lint.sh",
            "timeout": 30,
            "async": true
          }
        ]
      },
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/bun-test.sh",
            "timeout": 30,
            "async": true
          }
        ]
      },
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/swift-fmt.sh",
            "timeout": 10
          }
        ]
      },
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/swift-lint.sh",
            "timeout": 30,
            "async": true
          }
        ]
      },
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/swift-build.sh",
            "timeout": 120,
            "async": true
          }
        ]
      }
    ]
  },
  "permissions": {
    "allow": [
      "Bash(cargo *)",
      "Bash(git *)",
      "Bash(bun *)",
      "Bash(biome *)",
      "Bash(psql *)",
      "Bash(nix *)",
      "Bash(xcodebuild *)",
      "Bash(swiftformat *)",
      "Bash(swiftlint *)",
      "Bash(xcrun *)",
      "Bash(plutil *)",
      "Bash(scripts/*)",
      "Bash(rm *)",
      "Bash(ls *)",
      "Bash(wc *)",
      "Bash(jq *)",
      "WebSearch",
      "WebFetch(domain:github.com)",
      "WebFetch(domain:api.github.com)",
      "WebFetch(domain:raw.githubusercontent.com)",
      "WebFetch(domain:crates.io)",
      "WebFetch(domain:docs.rs)",
      "WebFetch(domain:lib.rs)",
      "WebFetch(domain:oat.ink)",
      "WebFetch(domain:biomejs.dev)",
      "WebFetch(domain:enablebanking.com)",
      "WebFetch(domain:developer.apple.com)"
    ]
  },
  "enabledPlugins": {
    "frontend-design@claude-plugins-official": true
  }
}
```

## Linter Configurations

**See `IOS_LINTER_CONFIG.md` for complete linter configuration philosophy and options.**

### Recommended Approach: Vanilla + Strict Mode (No Config Files)

Use tool defaults with strict mode flags instead of config files:

```bash
# SwiftLint - vanilla defaults + strict mode (no .swiftlint.yml needed)
swiftlint lint --strict

# SwiftFormat - vanilla defaults (no .swiftformat needed)
swiftformat --swiftversion 6 .

# Or use Xcode built-in (zero external dependencies)
xcrun swift-format lint --strict --recursive BudgetApp/
```

### Alternative: Minimal Config Files (If Required)

If Xcode integration requires config files, use minimal versions:

**`ios/.swiftlint.yml`** (optional, 7 lines):
```yaml
strict: true
included: [BudgetApp]
excluded: [.build, DerivedData, build]
# NO custom rules - use 100% defaults
```

**`ios/.swiftformat`** (optional, 3 lines):
```
--swiftversion 6
--exclude .build,DerivedData,build
# NO custom formatting - use 100% defaults
```

Both approaches provide **pedantic enforcement** via strict mode while using **vanilla tool defaults** with zero customization.

## Xcode Project Configuration

### `ios/ExportOptions.plist` (for IPA export in CI)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>ad-hoc</string>
    <key>teamID</key>
    <string>YOUR_TEAM_ID</string>
    <key>signingStyle</key>
    <string>manual</string>
    <key>provisioningProfiles</key>
    <dict>
        <key>com.budget.app</key>
        <string>Budget Ad Hoc</string>
    </dict>
</dict>
</plist>
```

### Shared Xcode Scheme

Create `ios/BudgetApp.xcodeproj/xcshareddata/xcschemes/BudgetApp.xcscheme` (committed to git):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Scheme
   LastUpgradeVersion = "1600"
   version = "1.7">
   <BuildAction
      parallelizeBuildables = "YES"
      buildImplicitDependencies = "YES">
      <BuildActionEntries>
         <BuildActionEntry
            buildForTesting = "YES"
            buildForRunning = "YES"
            buildForProfiling = "YES"
            buildForArchiving = "YES"
            buildForAnalyzing = "YES">
            <BuildableReference
               BuildableIdentifier = "primary"
               BlueprintIdentifier = "BudgetApp"
               BuildableName = "BudgetApp.app"
               BlueprintName = "BudgetApp"
               ReferencedContainer = "container:BudgetApp.xcodeproj">
            </BuildableReference>
         </BuildActionEntry>
      </BuildActionEntries>
   </BuildAction>
   <TestAction
      buildConfiguration = "Debug"
      selectedDebuggerIdentifier = "Xcode.DebuggerFoundation.Debugger.LLDB"
      selectedLauncherIdentifier = "Xcode.DebuggerFoundation.Launcher.LLDB"
      shouldUseLaunchSchemeArgsEnv = "YES"
      codeCoverageEnabled = "YES">
      <Testables>
         <TestableReference
            skipped = "NO">
            <BuildableReference
               BuildableIdentifier = "primary"
               BlueprintIdentifier = "BudgetAppTests"
               BuildableName = "BudgetAppTests.xctest"
               BlueprintName = "BudgetAppTests"
               ReferencedContainer = "container:BudgetApp.xcodeproj">
            </BuildableReference>
         </TestableReference>
      </Testables>
   </TestAction>
   <LaunchAction
      buildConfiguration = "Debug"
      selectedDebuggerIdentifier = "Xcode.DebuggerFoundation.Debugger.LLDB"
      selectedLauncherIdentifier = "Xcode.DebuggerFoundation.Launcher.LLDB"
      launchStyle = "0"
      useCustomWorkingDirectory = "NO"
      ignoresPersistentStateOnLaunch = "NO"
      debugDocumentVersioning = "YES"
      debugServiceExtension = "internal"
      allowLocationSimulation = "YES">
      <BuildableProductRunnable
         runnableDebuggingMode = "0">
         <BuildableReference
            BuildableIdentifier = "primary"
            BlueprintIdentifier = "BudgetApp"
            BuildableName = "BudgetApp.app"
            BlueprintName = "BudgetApp"
            ReferencedContainer = "container:BudgetApp.xcodeproj">
         </BuildableReference>
      </BuildableProductRunnable>
   </LaunchAction>
</Scheme>
```

## Required GitHub Secrets

For the release workflow to work, configure these secrets in GitHub repository settings:

- `IOS_PROVISIONING_PROFILE_BASE64`: Base64-encoded `.mobileprovision` file
- `IOS_CERTIFICATE_BASE64`: Base64-encoded `.p12` certificate
- `IOS_CERTIFICATE_PASSWORD`: Password for the `.p12` certificate

## Project Structure for CI

```
ios/
├── BudgetApp.xcodeproj/
│   ├── project.pbxproj
│   └── xcshareddata/
│       └── xcschemes/
│           └── BudgetApp.xcscheme  # Shared scheme (committed to git)
├── BudgetApp/
│   └── (app code)
├── BudgetAppTests/
│   └── (test code)
├── .swiftlint.yml                  # SwiftLint config
├── .swiftformat                    # SwiftFormat config
└── ExportOptions.plist             # IPA export settings for CI
```

## Installation Instructions

### Local Development Setup

```bash
# Install Xcode 16 (for iOS 26 SDK)
# Available from Mac App Store or developer.apple.com

# Install tools via Homebrew
brew install swiftformat
brew install swiftlint
brew install xcbeautify  # Optional: prettifies xcodebuild output

# Activate pre-commit hook
ln -sf ../../.github/hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit

# Make Claude hooks executable
chmod +x .claude/hooks/swift-*.sh
```

### First Build

```bash
cd ios
xcodebuild build \
  -scheme BudgetApp \
  -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=26.0' \
  -configuration Debug
```
