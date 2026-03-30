# Budget iOS App

Pure Swift iOS 26 app with Liquid Glass design.

## Creating the Xcode Project (On Mac)

1. **Open Xcode** on your Mac

2. **File → New → Project**

3. **Choose template:**
   - Platform: iOS
   - Template: App

4. **Project settings:**
   - Product Name: `Budget`
   - Team: Select your Apple ID (free account works)
   - Organization Identifier: `com.budget`
   - Bundle Identifier: `com.budget.Budget`
   - Interface: **SwiftUI**
   - Language: **Swift**
   - Storage: **None** (we'll use UserDefaults)
   - Include Tests: Uncheck

5. **Save location:**
   - Navigate to this `ios/` directory
   - Click "Create"

6. **Configure build settings:**
   - Select the Budget target
   - General tab:
     - Minimum Deployments: **iOS 26.0**
   - Build Settings tab:
     - Search for "Swift Language Version"
     - Set to: **Swift 6**
     - Search for "Strict Concurrency Checking"
     - Set to: **Complete**

7. **Commit and push:**
   ```bash
   git add ios/
   git commit -m "Add iOS 26 Xcode project"
   git push
   ```

## iOS 26 Liquid Glass Design Guidelines

### Core Principles
- **Glass needs a background**: Liquid Glass is translucent, it must have something behind it to look good
- **Use native controls**: SwiftUI controls automatically get Liquid Glass styling in iOS 26
- **Rounded, continuous corners**: Use `.clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))`
- **Material backgrounds**: Use `.ultraThinMaterial`, `.thinMaterial`, or `.regularMaterial`

### Example Card Component
```swift
VStack(alignment: .leading, spacing: 16) {
    Text("Title")
        .font(.title2)
        .fontWeight(.semibold)

    Text("Content")
        .font(.body)
        .foregroundStyle(.secondary)
}
.padding(24)
.frame(maxWidth: .infinity, alignment: .leading)
.background(.ultraThinMaterial)
.clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
.overlay {
    RoundedRectangle(cornerRadius: 24, style: .continuous)
        .strokeBorder(.white.opacity(0.2), lineWidth: 1)
}
.shadow(color: .black.opacity(0.1), radius: 20, y: 10)
```

### Background Layer
Always have a visual layer behind glass elements:
```swift
ZStack {
    // Background for glass to sample
    LinearGradient(
        colors: [.blue.opacity(0.3), .purple.opacity(0.3)],
        startPoint: .topLeading,
        endPoint: .bottomTrailing
    )
    .ignoresSafeArea()

    // Glass content on top
    YourGlassCard()
}
```

## Swift 6 Concurrency Best Practices

### Use @MainActor for ViewModels
```swift
@MainActor
final class BudgetViewModel: ObservableObject {
    @Published private(set) var transactions: [Transaction] = []

    func loadTransactions() async {
        // Network calls with async/await
        transactions = await api.fetchTransactions()
    }
}
```

### Structured Concurrency
```swift
// Don't use DispatchQueue or GCD
await withTaskGroup(of: Transaction.self) { group in
    for id in transactionIds {
        group.addTask {
            await api.fetchTransaction(id)
        }
    }
}
```

### Sendable Types
Make your models `Sendable` for safe concurrency:
```swift
struct Transaction: Codable, Sendable {
    let id: UUID
    let amount: Decimal
    let description: String
}
```

## Connecting to the Budget API

The backend runs at `http://localhost:3001` (or production server).

### Example API Client
```swift
actor BudgetAPI {
    private let baseURL: URL

    init(baseURL: URL = URL(string: "http://localhost:3001")!) {
        self.baseURL = baseURL
    }

    func fetchTransactions() async throws -> [Transaction] {
        let url = baseURL.appendingPathComponent("/transactions")
        let (data, _) = try await URLSession.shared.data(from: url)
        return try JSONDecoder().decode([Transaction].self, from: data)
    }
}
```

### Usage in ViewModel
```swift
@MainActor
final class TransactionsViewModel: ObservableObject {
    @Published private(set) var transactions: [Transaction] = []
    private let api = BudgetAPI()

    func load() async {
        do {
            transactions = try await api.fetchTransactions()
        } catch {
            print("Failed to load: \\(error)")
        }
    }
}
```

## Running Locally

1. **Start the backend:**
   ```bash
   cd /home/sam/projects/budget
   cargo run
   ```
   Server runs on `http://localhost:3001`

2. **Run the iOS app:**
   - Open `Budget.xcodeproj` in Xcode
   - Select iPhone simulator or physical device
   - Click Run (⌘R)

## Building for Release

### GitHub Actions
When you push to `main`, GitHub Actions automatically:
1. Builds the app for iOS 26
2. Creates an IPA file
3. Uploads to GitHub Releases as `ios-v<DATE>-<COMMIT>`

### Manual Build
```bash
xcodebuild \
  -project Budget.xcodeproj \
  -scheme Budget \
  -configuration Release \
  -sdk iphoneos \
  -destination 'generic/platform=iOS' \
  -archivePath Budget.xcarchive \
  archive

xcodebuild \
  -exportArchive \
  -archivePath Budget.xcarchive \
  -exportPath build \
  -exportOptionsPlist ExportOptions.plist
```

## Project Structure

Recommended organization:
```
ios/
├── Budget/
│   ├── BudgetApp.swift          # App entry point
│   ├── Models/
│   │   ├── Transaction.swift    # Data models (Sendable, Codable)
│   │   └── Account.swift
│   ├── ViewModels/
│   │   ├── TransactionsViewModel.swift  # @MainActor ObservableObject
│   │   └── AccountsViewModel.swift
│   ├── Views/
│   │   ├── TransactionsView.swift       # SwiftUI views
│   │   ├── AccountsView.swift
│   │   └── Components/
│   │       └── GlassCard.swift          # Reusable Liquid Glass components
│   ├── API/
│   │   └── BudgetAPI.swift              # actor for API calls
│   └── Assets.xcassets/
└── Budget.xcodeproj/
```

## Resources

- [iOS 26 Liquid Glass Design Guide](https://medium.com/@madebyluddy/overview-37b3685227aa)
- [Swift 6 Concurrency Guide](https://www.avanderlee.com/concurrency/approachable-concurrency-in-swift-6-2-a-clear-guide/)
