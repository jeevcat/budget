# iOS Linter Configuration - Vanilla + Strict Mode

**Philosophy**: Use tool defaults with strict/pedantic mode instead of custom configuration files.

## SwiftLint: Vanilla Defaults + Strict Mode

### Recommended Approach: No Config File

Run SwiftLint with `--strict` flag to use all default rules with warnings treated as errors:

```bash
swiftlint lint --strict
```

**Benefits**:
- Zero configuration files
- Uses well-tested SwiftLint defaults (follows Swift API Design Guidelines)
- Strict mode treats warnings as errors (pedantic enforcement)
- Automatically gets new rules in SwiftLint updates
- No maintenance burden from custom configs

### Alternative: Minimal Config File (If Required)

If you must have a `.swiftlint.yml` file (e.g., for Xcode integration), keep it minimal:

```yaml
# ios/.swiftlint.yml - Minimal config, vanilla defaults + strict mode
strict: true  # Treat all warnings as errors (pedantic mode)

# Paths only (required for Xcode)
included:
  - BudgetApp

excluded:
  - .build
  - DerivedData
  - build

# NO custom rules, NO disabled rules, NO threshold changes
# Use 100% SwiftLint defaults
```

**What This Enables**:
- All ~200 default SwiftLint rules
- Strict mode (warnings become errors)
- Standard thresholds (file length, line length, complexity)
- No customization = consistent with Swift community

### SwiftLint Version

Use SwiftLint 0.58.2+ (January 2026) for:
- Swift 6 support
- 30% performance improvements
- New rules: `async_without_await`, `unused_parameter`, `prefer_key_path`, `redundant_sendable`

## SwiftFormat: Vanilla Defaults

### Recommended Approach: No Config File

Run SwiftFormat with only Swift version specified:

```bash
swiftformat --swiftversion 6 .
```

**Benefits**:
- Zero configuration files
- Uses SwiftFormat community-approved defaults
- Only specifies Swift version (required for compatibility)
- Automatically gets formatting updates
- No bikeshedding over style preferences

### Alternative: Minimal Config File (If Required)

```
# ios/.swiftformat - Minimal config, vanilla defaults
--swiftversion 6

# Exclusions only
--exclude .build
--exclude DerivedData
--exclude build

# NO custom rules, NO formatting preferences
# Use 100% SwiftFormat defaults
```

## Xcode 16 Built-in swift-format (Zero Dependencies)

**Recommended**: Use Xcode's built-in `swift-format` instead of third-party SwiftFormat.

### Usage

```bash
# Lint (check for violations, strict mode)
xcrun swift-format lint --strict --recursive BudgetApp/

# Format (fix violations in-place)
xcrun swift-format format --in-place --recursive BudgetApp/
```

**Benefits**:
- No external dependencies (comes with Xcode 16)
- Apple-maintained (guaranteed Swift compatibility)
- Vanilla defaults (Apple's preferred style)
- No config file needed
- Strict mode built-in with `--strict` flag

**Note**: `swift-format` is different from SwiftFormat (third-party tool). Apple's `swift-format` uses different defaults but is the official Apple solution as of Xcode 16.

## Xcode Compiler Warnings (Maximum Strictness)

Enable strict compiler warnings in Xcode build settings (no config file):

### Build Settings

**Swift Compiler - Warnings Policies**
- Treat Warnings as Errors: **Yes**

**Swift Compiler - Code Generation**
- Swift Concurrency Checking: **Complete** (Swift 6 strict concurrency)
- Sendable Checking: **Complete**

**Swift Compiler - Warning Policies (All Defaults)**
- Use Xcode defaults - Apple curates these for best practices
- All warnings enabled by default

**Additional Recommendations**:
- Enable "Run Script Phases" for SwiftLint in Build Phases
- Use "Analyze" (⌘+Shift+B) regularly for deeper static analysis

## CI/CD Integration

### GitHub Actions (Vanilla + Strict)

```yaml
- name: SwiftFormat check (vanilla)
  run: swiftformat --lint --swiftversion 6 .

- name: SwiftLint (strict mode)
  run: swiftlint lint --strict

- name: Build with warnings as errors
  run: |
    xcodebuild build \
      -scheme BudgetApp \
      -configuration Debug \
      GCC_TREAT_WARNINGS_AS_ERRORS=YES
```

### Git Pre-Commit Hook

```bash
# SwiftFormat - vanilla defaults
swiftformat --lint --swiftversion 6 ios/

# SwiftLint - strict mode (warnings as errors)
cd ios && swiftlint lint --strict
```

### Claude Hooks

```bash
# swift-fmt.sh - vanilla defaults
swiftformat --swiftversion 6 "$FILE_PATH"

# swift-lint.sh - strict mode
cd ios && swiftlint lint --path "$FILE_PATH" --strict
```

## Summary: Zero Config Philosophy

### What We Use
1. **SwiftLint**: `swiftlint lint --strict` (no config file)
2. **SwiftFormat**: `swiftformat --swiftversion 6 .` (no config file)
3. **Xcode**: Treat warnings as errors + Swift 6 strict concurrency
4. **Alternative**: Xcode built-in `xcrun swift-format --strict` (zero dependencies)

### What We Don't Use
- ❌ Custom SwiftLint rules
- ❌ Disabled default rules
- ❌ Custom threshold tweaks
- ❌ SwiftFormat style preferences
- ❌ Complex configuration files

### Result
- **Maximum strictness** (pedantic mode via `--strict`)
- **Vanilla defaults** (zero customization)
- **Zero or minimal config files**
- **Community-standard enforcement**
- **Automatic updates** when tools improve

## Tool Installation

```bash
# Option 1: Use Homebrew (for third-party tools)
brew install swiftlint
brew install swiftformat

# Option 2: Use only Xcode built-ins (zero external deps)
# swift-format is included in Xcode 16
# SwiftLint still needs Homebrew (no Xcode equivalent yet)
```

## Running Locally

```bash
# Vanilla SwiftFormat with strict mode
swiftformat --swiftversion 6 --lint .

# Vanilla SwiftLint with strict mode
swiftlint lint --strict

# Or use Xcode built-in (Apple's defaults)
xcrun swift-format lint --strict --recursive BudgetApp/
```

All violations become build errors. No custom rules. Pure pedantic vanilla defaults.
