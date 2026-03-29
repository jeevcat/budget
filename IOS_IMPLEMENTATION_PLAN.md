# iOS Implementation Plan

Pure native iOS app targeting iOS 26+ with Liquid Glass design system, modern SwiftUI, and Swift 6 strict concurrency.

## Project Architecture

### Target Platform
- Minimum iOS version: 26.0 (current as of March 2026)
- Target devices: iPhone only
- Language: Swift 6 with strict concurrency enabled
- UI Framework: SwiftUI with iOS 26 Liquid Glass APIs
- Primary API: Observation framework for state management
- Secondary API: Swift Concurrency (async/await, actors) for networking and background work
- Design System: Liquid Glass (Apple's unified translucent material system introduced WWDC 2025)

### Liquid Glass Design Philosophy
Liquid Glass is a translucent, dynamic material that reflects and refracts surroundings while transforming to bring focus to content. It provides:
- Optical properties of glass with sense of fluidity
- Real-time rendering with specular highlights that react to movement
- Blur of content behind with color and light reflection
- Interactive touch and pointer response
- First unified design language across all Apple platforms (iOS 26, iPadOS 26, macOS Tahoe 26, watchOS 26, tvOS 26)

### Project Structure
```
BudgetApp/
├── BudgetApp.xcodeproj
├── BudgetApp/
│   ├── App/
│   │   ├── BudgetApp.swift                 # App entry point
│   │   └── Configuration.swift             # App-wide config (API base URL)
│   ├── Models/
│   │   ├── Domain/
│   │   │   ├── Transaction.swift
│   │   │   ├── Category.swift
│   │   │   ├── BudgetMonth.swift
│   │   │   ├── BudgetStatus.swift
│   │   │   ├── ProjectStatus.swift
│   │   │   ├── LedgerSummary.swift
│   │   │   ├── CashFlowItem.swift
│   │   │   └── Enums.swift                 # BudgetMode, BudgetType, PaceIndicator, CategoryMethod
│   │   └── API/
│   │       ├── Requests/
│   │       │   ├── CategoryRequest.swift
│   │       │   ├── CategorizeRequest.swift
│   │       │   └── ConnectionTestRequest.swift
│   │       └── Responses/
│   │           ├── StatusResponse.swift
│   │           ├── TransactionPage.swift
│   │           └── ConnectionResult.swift
│   ├── Services/
│   │   ├── Networking/
│   │   │   ├── APIClient.swift             # Core HTTP client using URLSession
│   │   │   ├── APIEndpoint.swift           # Endpoint definitions
│   │   │   ├── APIError.swift              # Error types
│   │   │   └── RequestBuilder.swift        # URL request construction
│   │   └── Persistence/
│   │       └── ConfigStore.swift           # Keychain wrapper for server config (secure)
│   ├── ViewModels/
│   │   ├── SetupViewModel.swift            # Observable for setup flow
│   │   ├── DashboardViewModel.swift        # Observable for dashboard
│   │   ├── TransactionsViewModel.swift     # Observable for transactions list
│   │   ├── TransactionDetailViewModel.swift
│   │   ├── CategoriesViewModel.swift
│   │   ├── CategoryEditViewModel.swift
│   │   └── SettingsViewModel.swift         # Observable for settings (disconnect action)
│   ├── Views/
│   │   ├── Setup/
│   │   │   └── SetupView.swift             # Form-based setup (SwiftUI Form)
│   │   ├── Dashboard/
│   │   │   ├── DashboardView.swift
│   │   │   ├── DashboardTabContent.swift
│   │   │   ├── MonthNavigator.swift
│   │   │   ├── AnnualHeader.swift
│   │   │   ├── SummaryCards.swift
│   │   │   ├── CategoryRow.swift
│   │   │   ├── SpendBar.swift
│   │   │   ├── PaceBadge.swift
│   │   │   ├── CategoryDetailView.swift
│   │   │   └── ProjectDrillDownView.swift
│   │   ├── Transactions/
│   │   │   ├── TransactionsView.swift
│   │   │   ├── TransactionRow.swift
│   │   │   └── TransactionDetailView.swift
│   │   ├── Categories/
│   │   │   ├── CategoriesView.swift        # List + nav bar button (no FAB)
│   │   │   ├── CategoryGroupCard.swift
│   │   │   └── CategoryEditView.swift      # SwiftUI Form (automatic styling)
│   │   ├── Settings/
│   │   │   └── SettingsView.swift          # SwiftUI Form (server info, disconnect)
│   │   ├── Components/
│   │   │   ├── StatCard.swift
│   │   │   ├── BudgetModeIndicator.swift
│   │   │   └── CategoryPicker.swift
│   │   └── Root/
│   │       ├── ContentView.swift           # Root navigation container
│   │       └── AppTabView.swift            # Bottom tab bar (4 tabs, Liquid Glass)
│   ├── Utilities/
│   │   ├── Extensions/
│   │   │   ├── Date+Formatting.swift
│   │   │   ├── Decimal+Formatting.swift
│   │   │   └── Color+Theme.swift
│   │   ├── Formatters/
│   │   │   ├── AmountFormatter.swift
│   │   │   └── DateFormatters.swift
│   │   └── Haptics/
│   │       └── HapticEngine.swift          # Wrapper for UIFeedbackGenerator
│   └── Resources/
│       ├── Assets.xcassets
│       └── Info.plist
└── BudgetAppTests/
    ├── ViewModelTests/
    ├── ServiceTests/
    └── MockData/
```

## Technology Stack

### Core Frameworks
- **SwiftUI**: All UI rendering with Liquid Glass materials
- **Observation**: State management via `@Observable` macro on ViewModels
- **Swift Concurrency**: All async operations (network, heavy computation)
- **Foundation**: URLSession for networking, Keychain for secure config persistence
- **UIKit**: UIFeedbackGenerator for haptics only

### No Third-Party Dependencies
All functionality implemented using native iOS APIs exclusively.

## Liquid Glass Implementation Strategy

### Automatic Adoption
When recompiled with Xcode 26 SDK, these components automatically get Liquid Glass:
- NavigationBar (transparent with glass buttons)
- TabBar (capsule-shaped, inset from edges, floats above content)
- Toolbar
- Sheets, Popovers, Menus, Alerts
- Search bars
- Toggles, Sliders, Pickers (during interaction)

**No code changes required** for basic adoption - simply compile with Xcode 26.

### Manual Application for Custom Views
Use `.glassEffect()` modifier for custom components:

#### API Signature
```swift
func glassEffect<S: Shape>(
    _ glass: Glass = .regular,
    in shape: S = .capsule,
    isEnabled: Bool = true
) -> some View
```

#### Glass Variants
- `.regular` - Standard glass effect (default)
- `.clear` - More transparent variant
- `.identity` - No effect (for conditional toggling)

#### Glass Modifiers (chainable)
- `.tint(_ color: Color)` - Semantic color tinting for primary actions or states
- `.interactive()` - Enables scaling, bounce, and shimmer effects

#### Shape Options
- `.capsule` (default)
- `.circle`
- `.ellipse`
- `.rect(cornerRadius:)` with `.containerConcentric` for adaptive corners

### Liquid Glass Usage Guidelines (Apple Best Practices)

#### Where to Use
- **Navigation layer components** that float above content:
  - Toolbars, tab bars, navigation bars
  - Floating action buttons
  - Glass cards and modals
  - Blurred background zones for critical actions
  - Category picker bottom sheet
  - Stat cards on dashboard

#### Where NOT to Use
- Content layer (transaction lists, budget tables, text)
- Full-screen backgrounds
- Scrollable content items
- Stacked glass layers (avoid z-fighting)
- Everywhere (use sparingly on key surfaces only)

#### Opacity Recommendations
- **70% opacity** - Supporting text, secondary buttons, navigation tabs (visible but not dominant)
- **40% opacity** - Decorative UI (dividers, outlines, icons)

### Liquid Glass Best Practices for This App

#### Cards (StatCard, CategoryRow, CategoryGroupCard)
Apply `.glassEffect()` to card containers:
```swift
VStack {
    // card content
}
.glassEffect(.regular, in: .rect(cornerRadius: 12))
```

#### Tab Bar
Automatically glass-styled when compiled with Xcode 26 SDK. Tab bar will:
- Be capsule-shaped and inset from screen edges
- Float above content with glass material
- Minimize on scroll (only active tab visible)
- Expand when user scrolls back up

#### Navigation Components (MonthNavigator, FloatingActionButton)
Apply interactive glass for touch responsiveness:
```swift
Button(action: { /* ... */ }) {
    Image(systemName: "plus")
}
.glassEffect(.regular.tint(.blue).interactive())
```

#### Bottom Sheets (CategoryPicker)
Presented sheets automatically receive glass treatment. Enhance with explicit glass on picker content:
```swift
VStack {
    // picker content
}
.glassEffect(.clear)
```

#### Spend Bar (Custom Component)
Custom-drawn component using Canvas with layered glass effect:
- Background track uses `.glassEffect(.clear)` for subtle depth
- Filled portion uses solid color (no glass to maintain readability)
- Budget mark line remains opaque for clarity

### Accessibility Considerations for Liquid Glass
- **Reduce Transparency**: Detect and respect accessibility setting to disable glass when enabled
- **Contrast**: Ensure 70%+ opacity for all text on glass surfaces
- **Fallback**: Use solid backgrounds when Reduce Transparency is active

## State Management

### Architecture Pattern
Pure MVVM with Swift 6 Observation framework. No Combine, no `@Published`, no `ObservableObject`.

### Why @Observable (not ObservableObject)
- **iOS 17+ standard**: `@Observable` macro is the modern replacement for `ObservableObject`
- **No property wrappers needed**: All stored properties automatically observable (no `@Published`)
- **Granular updates**: Views only re-render when properties they read change (more efficient)
- **Swift 6 compatible**: Fully integrated with Swift concurrency
- **Use `@State` not `@StateObject`**: Views use `@State` to hold ViewModels (not `@StateObject`)

**CRITICAL**: `@State` with `@Observable` re-creates ViewModels on view rebuild. For ViewModels that should survive view rebuilds (like tab ViewModels), use `@State` at the parent level and pass down via `@Environment` or direct parameter.

### ViewModel Design Principles (Swift 6.2 Best Practices)
Each ViewModel is an `@Observable` class that:
- **Class-scoped `@MainActor`**: Entire ViewModel runs on main thread (simplifies UI code)
- **All properties auto-tracked**: No `@Published` needed
- **Async methods for actions**: Use async/await for API calls
- **No SwiftUI imports**: Only import Foundation (keeps ViewModels testable)
- **Dependencies injected**: APIClient passed in init
- **Sendable conformance**: Only when needed for cross-actor communication

### Example ViewModel Structure (Swift 6 + iOS 26)
```swift
import Foundation

@Observable
@MainActor
final class DashboardViewModel {
    // State properties (automatically observable via @Observable macro)
    var currentMonth: BudgetMonth?
    var monthlyStatuses: [BudgetStatus] = []
    var projects: [ProjectStatus] = []
    var selectedCategoryId: String?
    var selectedTab: BudgetMode = .monthly
    var loading: Bool = false
    var error: String?

    // Dependencies (injected, private)
    private let apiClient: APIClient

    init(apiClient: APIClient) {
        self.apiClient = apiClient
    }

    // Async actions (run on MainActor automatically)
    func loadStatus() async {
        loading = true
        defer { loading = false }

        do {
            let response = try await apiClient.getStatus(monthId: currentMonth?.id)
            monthlyStatuses = response.monthly
            projects = response.projects
        } catch {
            self.error = error.localizedDescription
        }
    }

    func goToNextMonth() async {
        // Implementation
    }

    // Synchronous actions (no await needed)
    func selectCategory(_ id: String?) {
        selectedCategoryId = id
    }

    func selectTab(_ mode: BudgetMode) {
        selectedTab = mode
    }
}
```

### View Usage with @State
```swift
struct DashboardView: View {
    @State private var viewModel: DashboardViewModel

    init(apiClient: APIClient) {
        // Initialize ViewModel in init, not inline
        _viewModel = State(initialValue: DashboardViewModel(apiClient: apiClient))
    }

    var body: some View {
        // View automatically observes viewModel changes
        // Only re-renders when properties used in body change
        List(viewModel.monthlyStatuses) { status in
            Text(status.name)
        }
        .task {
            await viewModel.loadStatus()
        }
    }
}
```

### Swift 6.2 Concurrency Patterns

#### Progressive Disclosure Approach
Start simple, add complexity only when needed:
1. **Sequential code first**: Write synchronous code
2. **Add async/await**: For API calls and suspending operations
3. **Use `@MainActor`**: For UI-bound ViewModels (class-scoped)
4. **Use `actor`**: Only for shared mutable state (rare in MVVM)

#### MainActor Usage
- **ViewModel-level**: Apply `@MainActor` to entire ViewModel class for UI-bound logic
- **Method-level**: Apply to individual methods only if ViewModel has mixed UI/background work
- **Automatic isolation inheritance**: Swift 6.2 async functions inherit caller's isolation

#### Avoid Over-Isolation
Don't mark APIClient or network layer as `@MainActor` - keep them isolation-free for flexibility.

### Data Flow
1. View holds ViewModel via `@State` (for simple views) or receives via `@Environment`
2. SwiftUI observes ViewModel automatically (no `@ObservedObject` needed)
3. User interaction triggers ViewModel async method
4. ViewModel updates properties (on MainActor)
5. SwiftUI re-renders only views reading changed properties

## Networking Layer

### API Client Architecture
Single `APIClient` class using URLSession with modern async/await APIs.

### Core Components
- **APIClient**: Manages URLSession, handles authentication, executes requests
- **APIEndpoint**: Enum defining all backend endpoints with associated types for request/response
- **RequestBuilder**: Constructs URLRequest from endpoint + parameters
- **APIError**: Typed error cases (network failure, auth failure, decode failure, server error)

### Authentication
Bearer token stored in Keychain (secure), injected into Authorization header on every request.

### Response Handling
All responses decoded using JSONDecoder with snake_case to camelCase key conversion strategy.

### Error Handling Strategy
- Network errors mapped to user-friendly messages
- 401/403 trigger logout flow (clear config, return to setup)
- 404 shown as "not found" error in UI
- 400 with error JSON body decoded and displayed
- Connection failures shown with retry option

### Request/Response Types
Each API endpoint has corresponding Codable request/response types mirroring the Rust backend schemas.

## Screens and Navigation

### App Structure
Single `NavigationStack` per tab with Liquid Glass tab bar for top-level routes.

**Why NavigationStack (not NavigationSplitView)**:
- iPhone-only app doesn't need multi-column layout
- NavigationStack is idiomatic for iPhone single-column hierarchical navigation
- NavigationSplitView auto-collapses on iPhone but has different transitions and APIs
- NavigationStack is simpler and matches iOS conventions for phone apps

### Navigation Model
Type-safe navigation using SwiftUI NavigationStack with Hashable route types (not Codable - unnecessary).

### Routes (per tab, using Hashable not Codable)
```swift
// Each tab has its own navigation path
enum DashboardRoute: Hashable {
    case categoryDetail(id: String)
    case projectDrillDown(id: String)
}

enum TransactionRoute: Hashable {
    case detail(id: String)
}

enum CategoryRoute: Hashable {
    case edit(id: String?)  // nil for create new
}

// Settings tab has no navigation (single screen)
```

**Note**: Routes use `Hashable` (required for NavigationStack), not `Codable`. Codable is only needed for persistence/deep linking, which we don't have.

### Tab Bar Items (Liquid Glass Automatic)
Four tabs with iOS 26 glass-styled tab bar (idiomatic iOS - settings in tabs):
1. **Budget** (SF Symbol: chart.bar.fill) → DashboardView
2. **Transactions** (SF Symbol: doc.text.fill) → TransactionsView with badge showing uncategorized count
3. **Categories** (SF Symbol: folder.fill) → CategoriesView
4. **Settings** (SF Symbol: gearshape.fill) → SettingsView with disconnect action

Tab bar behavior:
- Capsule-shaped, inset from edges
- Floats above content with Liquid Glass
- Minimizes on scroll (shows only active tab)
- Expands on scroll up
- Four tabs creates balanced layout (iOS preference over three)

**Categories View Navigation Pattern**:
Since we're using tab bar for navigation, Categories view should use **nav bar trailing button** for add action (not bottom toolbar, per Apple HIG: "toolbar OR tab bar, not both")

### Screen Hierarchy

#### Setup Flow (pre-authentication)
- **SetupView**: Server URL + API key input, connection test, save config
  - Text field for server URL (with placeholder)
  - Secure text field for API key
  - Connect button (disabled during test, glass effect)
  - Error message display
  - Loading indicator during connection test

#### Dashboard Tab
- **DashboardView**: Picker with `.segmentedPickerStyle()` for mode selection (NOT Material PrimaryTabRow)
  - **Monthly**: Month navigator (glass buttons), summary cards (glass), category rows with pace indicators
  - **Annual**: Year header, summary cards (glass), annual category rows
  - **Projects**: Summary cards (glass), active projects list, finished projects collapsible section
  - **Use Picker, NOT custom tab row** - segmented control is idiomatic iOS for view switching within a screen
- **CategoryDetailView**: Sheet presentation (NOT push) with spend bar, pace badge, transaction list
  - Modal sheet for focused context
  - Dismiss with swipe or close button
  - Push navigation reserves for hierarchical drill-down only
- **ProjectDrillDownView**: Sheet presentation with project header, children breakdown, transaction list

#### Transactions Tab
- **TransactionsView**: List of uncategorized transactions with search, categorize action
  - Header showing count of uncategorized
  - Loading indicator when categorizing in background
  - Transaction rows (no glass - content layer)
  - Pull-to-refresh to reload
  - Backend search via API query parameter

#### Transaction Detail (pushed from any transaction tap)
- **TransactionDetailView**: Full transaction details, category assignment, AI suggestion acceptance
  - Hero section: large amount, merchant name, date (glass card)
  - Details card with all transaction metadata (glass)
  - AI suggestion card (if present) with accept button (glass with interactive)
  - Category section with current category, change/clear actions
  - Bottom sheet for category picker (glass)

#### Categories Tab
- **CategoriesView**: Hierarchical category tree with nav bar add button
  - Grouped by root categories in collapsible cards (glass)
  - Each category shows name, budget mode indicator, budget amount
  - Tap category to edit
  - **Nav bar trailing button** (glass + interactive, NOT Material FAB)
  - iOS 26 pattern: "toolbar OR tab bar" - since we have tab bar, use nav bar button

#### Settings Tab
- **SettingsView**: SwiftUI Form with app configuration and disconnect action
  - **Connected Server** section: Server URL (read-only, monospaced font)
  - **Actions** section: Disconnect button (destructive style, confirmation alert)
  - **About** section: App version, build number
  - Form automatic glass styling, grouped style
  - Disconnect shows confirmation alert before clearing Keychain and returning to setup

#### Category Edit (pushed or presented)
- **CategoryEditView**: SwiftUI Form (automatic platform styling, not custom VStack)
  - **Use native Form container** - follows HIG, automatic Liquid Glass styling
  - Name text field (Form-styled)
  - Parent category picker row (tappable, navigates to picker sheet)
  - Budget mode Picker with `.segmentedPickerStyle()` (automatic glass)
  - Budget type Picker with `.segmentedPickerStyle()` (automatic glass)
  - Budget amount text field with `.keyboardType(.decimalPad)`
  - Project date pickers (DatePicker with `.graphical` style for iOS 26)
  - Save button in nav bar (glass button, automatic)
  - Form provides grouped rows, proper spacing, platform-appropriate styling

## UI Components

### Design System

#### Color Palette
iOS 26 dynamic colors with Liquid Glass tinting:
- **Pace Colors**: pending (gray), under budget (green), on track (blue), above pace (yellow), over budget (red)
- **Budget Mode Colors**: monthly (crystal blue), annual (violet), project (yellow), salary (green), transfer (gray)
- **Accent Colors**: expense (red), income (green)

All colors support light/dark mode via iOS dynamic color system and work with glass tinting.

#### Typography
iOS system font (San Francisco) with dynamic type support:
- `.largeTitle` for hero amounts
- `.title` for section headers
- `.headline` for category names
- `.body` for transaction rows
- `.caption` for metadata

#### Spacing
Consistent spacing scale: 4, 8, 12, 16, 24, 32 points.

### Reusable Components with Liquid Glass

#### StatCard
Glass card showing label + large value, optional color, optional tap action.
```swift
VStack {
    Text(label).font(.caption)
    Text(value).font(.title).bold()
}
.padding()
.glassEffect(.regular, in: .rect(cornerRadius: 12))
```
Used for budget summary (total budget, total spent, remaining, categories over).

#### CategoryRow
Glass card showing category name, spent amount, spend bar with pace indicator, budget amount, remaining amount with color.
```swift
VStack {
    // category content
}
.glassEffect(.regular, in: .rect(cornerRadius: 12))
```
Supports subtitle for project date range. Supports selected state for navigation (uses `.tint()` modifier).

#### SpendBar
Custom-drawn horizontal progress bar with:
- Background track (glass with `.clear` variant for subtle depth)
- Filled portion (solid color for readability, no glass)
- Vertical budget mark line (opaque)
- Calculated width based on max value across all categories

Implementation: Custom view using Canvas for drawing with glass background layer.

#### PaceBadge
Small pill badge with pace label + optional delta amount.
```swift
Text(paceLabel)
    .font(.caption2)
    .padding(.horizontal, 8)
    .padding(.vertical, 4)
    .glassEffect(.regular.tint(paceColor), in: .capsule)
```

#### BudgetModeIndicator
Small colored circle + mode label (Monthly/Annual/Project/Salary/Transfer).
Circle uses solid color (no glass). Label uses system text.

#### TransactionRow
List row showing merchant name, date, amount, optional category badge, optional AI suggestion indicator.
No glass applied - this is content layer. Uses standard List row styling.

#### MonthNavigator
Header with prev/next buttons (glass + interactive), centered month range label, time remaining label.
```swift
HStack {
    Button { /* prev */ }
        .glassEffect(.regular.interactive())
    VStack { /* month labels */ }
    Button { /* next */ }
        .glassEffect(.regular.interactive())
}
```

#### CategoryPicker
Searchable list in bottom sheet (automatic glass) with hierarchical category display, checkmark for selected, indentation for children.
Sheet receives glass automatically. List items are content (no glass).

#### Toolbar Add Button (iOS 26 Pattern, NOT Material FAB)
iOS 26 uses floating bottom toolbar with primary action button, NOT circular FAB.
```swift
// In CategoriesView - use .toolbar instead of separate FAB
.toolbar {
    ToolbarItem(placement: .bottomBar) {
        Button(action: onAdd) {
            Label("Add Category", systemImage: "plus")
        }
        .glassEffect(.regular.tint(.blue).interactive())
    }
}
```
**Important**: iOS 26 pattern is toolbar OR tab bar, not both. Categories view should use List with toolbar, not TabView with FAB.

**Alternative for tab-based navigation**: Use nav bar trailing button instead:
```swift
.toolbar {
    ToolbarItem(placement: .navigationBarTrailing) {
        Button(action: onAdd) {
            Image(systemName: "plus")
        }
        .glassEffect(.regular.tint(.blue).interactive())
    }
}
```

### Bottom Sheets vs. Navigation
- Category picker: bottom sheet (automatic glass backdrop, quick selection, dismissible)
- Category edit: pushed navigation (full form, back button)
- Transaction detail: pushed navigation (full detail view)

## Data Models

### Domain Models
All models are `Codable` structs mirroring the API response types with appropriate snake_case to camelCase key mapping.

### Enums
All enums use `String` raw values matching the API serialization format.

#### BudgetMode
```
enum BudgetMode: String, Codable, CaseIterable {
    case monthly, annual, project, salary, transfer
}
```

#### BudgetType
```
enum BudgetType: String, Codable, CaseIterable {
    case fixed, variable
}
```

#### PaceIndicator
```
enum PaceIndicator: String, Codable {
    case pending
    case underBudget = "under_budget"
    case onTrack = "on_track"
    case abovePace = "above_pace"
    case overBudget = "over_budget"
}
```

#### CategoryMethod
```
enum CategoryMethod: String, Codable {
    case manual, rule, llm
}
```

### Model Relationships
Models contain IDs as strings (UUIDs), not nested objects. ViewModels resolve relationships when needed for display.

### Optional Fields
All optional fields in the API are optional properties in Swift models.

## Persistence

### Configuration Storage
Keychain for server URL and API key using a `ConfigStore` class wrapping KeychainAccess.

#### ConfigStore Interface
```
struct ServerConfig: Codable {
    let serverUrl: String
    let apiKey: String
}

@MainActor
final class ConfigStore {
    func load() -> ServerConfig?
    func save(_ config: ServerConfig)
    func clear()
}
```

Implementation uses native Keychain Services API (no third-party dependencies).

### No Local Database
All data fetched fresh from API on each screen load. No CoreData, no SQLite, no caching beyond in-memory ViewModel state.

### No Background Sync
App only fetches when user navigates to a screen. No background fetch, no push notifications.

## Formatting and Localization

### Amount Formatting
Decimal values formatted with appropriate currency symbol and decimal places using NumberFormatter.

#### Formatting Rules
- Default: 2 decimal places, no sign
- `showSign: true`: prefix with + or - based on sign
- Currency symbol determined by account/transaction currency field
- Thousands separators based on device locale

### Date Formatting
Multiple formatters for different contexts:
- **Short**: "Jan 15" (transaction rows)
- **Long**: "January 15, 2026" (detail views)
- **Month Range**: "Jan 1 – Jan 31, 2026" (dashboard header)

All formatters respect device locale for month names and date ordering.

### No Localization Initially
All strings hardcoded in English. No NSLocalizedString, no strings files.

## Haptic Feedback

### Implementation
Use UIFeedbackGenerator for tactile feedback on key interactions.

### Feedback Types and Usage
1. **UIImpactFeedbackGenerator** (.light, .medium, .heavy)
   - Transaction category assignment: .medium impact
   - Pull-to-refresh completion: .light impact
   - Button taps on glass buttons: .light impact

2. **UINotificationFeedbackGenerator** (.success, .warning, .error)
   - Successful connection test: .success
   - API error: .error
   - Category save success: .success

3. **UISelectionFeedbackGenerator**
   - Tab switching: selection feedback
   - Month navigation: selection feedback

### Best Practices
- Call `prepare()` method before triggering for minimal latency
- Use feedback consistently throughout app
- Respect system haptic settings (feedback disabled when haptics off)
- Don't overuse - only for significant interactions

### HapticEngine Wrapper
Create utility class to manage generator lifecycle and preparation:
```
@MainActor
final class HapticEngine {
    static let shared = HapticEngine()

    func impact(_ style: UIImpactFeedbackGenerator.FeedbackStyle)
    func notification(_ type: UINotificationFeedbackGenerator.FeedbackType)
    func selection()
}
```

## API Integration

### Endpoints Used

#### Authentication
- `GET /health` (unauthenticated health check)
- `GET /api/jobs/counts` (auth verification endpoint)

#### Setup Flow
- Connection test: health check + auth verification

#### Dashboard
- `GET /api/budgets/status?month_id={id}` → StatusResponse (entire dashboard state in one call)
- `GET /api/budgets/months` → [BudgetMonth] (for month navigation)

#### Transactions
- `GET /api/transactions?limit=50&offset=0&category_method=__none&search={query}` → TransactionPage
  - Backend supports search query parameter
  - Filter for uncategorized using `category_method=__none`
- `GET /api/transactions/{id}` → Transaction (single transaction detail)
- `POST /api/transactions/{id}/categorize` with body `{category_id}` (assign category)
- `DELETE /api/transactions/{id}/categorize` (clear category)

#### Categories
- `GET /api/categories` → [Category] (all categories with counts)
  - No backend search endpoint
  - Use client-side filtering for category picker search
- `POST /api/categories` with CreateCategory body → Category (create new)
- `PUT /api/categories/{id}` with CreateCategory body → Category (update existing)

#### Background Jobs
- `POST /api/jobs/categorize` (trigger AI categorization job, returns 202)
  - Job runs async on backend
  - App relies on pull-to-refresh to see results (no polling)

### Request Headers
All authenticated requests include:
```
Authorization: Bearer {apiKey}
Content-Type: application/json (for POST/PUT)
Accept: application/json
```

### Pagination Strategy
Automatic offset-based pagination for transactions (iOS-native UX):
- Load 50 at a time automatically
- Use `.onAppear` on last visible List item to trigger next page load
- Show `ProgressView` at bottom during fetch
- Track offset in ViewModel state (increment by 50 on each load)
- NO "Load More" button - iOS users expect automatic pagination

Note: Backend supports offset pagination currently. Cursor-based migration tracked in TODO.md but not implemented yet.

### Error Response Handling
Errors returned as JSON `{error: "message"}` with appropriate HTTP status. Decode and display error message to user.

## Testing Strategy

### Unit Tests
Test ViewModels in isolation using mock APIClient.

#### What to Test
- ViewModel state transitions
- Error handling paths
- Data transformation logic
- Formatters and utility functions

#### Mock Strategy
Protocol-based APIClient abstraction with mock implementation for tests.

### UI Tests
None initially. Consider adding XCUITest for critical flows later.

### Manual Testing Checklist
- Setup flow: invalid URL, invalid API key, network failure, successful connection
- Dashboard: month navigation, tab switching, category selection, drill-down, glass effects
- Transactions: list loading, pagination, search, category assignment, detail view
- Categories: tree expansion, create, edit with all mode combinations
- Haptics: verify feedback on all interactions
- Accessibility: test with Reduce Transparency enabled, verify glass fallbacks

## Build Configuration

### Xcode Project Settings
- Deployment target: iOS 26.0
- Swift version: Swift 6
- Swift concurrency checking: Complete
- Sendable checking: Complete
- No Objective-C bridging
- Enable complete strict concurrency

### Build Configurations
- Debug: localhost API base URL for development
- Release: production API base URL

### Info.plist Requirements
- `NSAppTransportSecurity` exception for localhost (Debug only)
- Display name: "Budget"
- Bundle identifier: `com.budget.app`
- Minimum OS version: 26.0
- Supported interface orientations: Portrait only
- Privacy - Keychain access description (for secure config storage)

### No Additional Capabilities Required
- No push notifications
- No background modes
- No app groups
- No extensions

## Deployment Considerations

### TestFlight Distribution
Standard App Store Connect flow for internal testing.

### App Store Submission
Not planned initially; this is a personal app.

### Version Management
Manual version bumps in Xcode project settings.

## Implementation Phases

### Phase 1: Foundation
1. Create Xcode project with iOS 26 target
2. Enable Swift 6 complete concurrency checking in build settings
3. Set up project structure with all directories
4. Implement ConfigStore for Keychain persistence
5. Implement APIClient with async/await (no `@MainActor`, keep isolation-free)
6. Define all domain models matching API schemas (structs with `Codable`)
7. Implement all formatters (amount, date) as structs or static methods
8. Implement HapticEngine wrapper (class with `@MainActor`)

### Phase 2: Setup Flow
1. Implement SetupViewModel with connection test logic
2. Implement SetupView with form, loading states, and glass button
3. Test full setup flow with mock and real backend
4. Add haptic feedback for connection success/failure

### Phase 3: Core Navigation
1. Implement ContentView with NavigationStack
2. Implement AppTabView with bottom tab bar (automatic glass)
3. Verify tab bar glass styling and minimization on scroll
4. Implement navigation between tabs with selection haptics
5. Implement logout action (clears config, returns to setup)

### Phase 4: Dashboard
1. Implement DashboardViewModel with status loading
2. Implement DashboardView with Picker (segmented style) for Monthly/Annual/Projects
3. Implement MonthNavigator component with glass buttons and haptics
4. Implement SummaryCards component with glass effect
5. Implement CategoryRow component with glass, SpendBar, and PaceBadge
6. Implement CategoryDetailView as sheet presentation (NOT push navigation)
7. Implement ProjectDrillDownView as sheet presentation
8. Test month navigation, segmented control switching, category selection
9. Verify glass effects on all cards
10. Test sheet dismissal gestures
11. Test Reduce Transparency fallback

### Phase 5: Transactions
1. Implement TransactionsViewModel with automatic pagination on scroll
2. Implement TransactionsView with list, search field, and loading states
3. Implement backend search via API query parameter
4. Implement automatic pagination using `.onAppear` on last item (NO "Load More" button)
5. Implement TransactionRow component (no glass - content)
6. Implement TransactionDetailViewModel
7. Implement TransactionDetailView as sheet with glass cards and haptics
8. Implement CategoryPicker bottom sheet with glass and client-side filtering
9. Test categorization flow, suggestion acceptance with haptics
10. Test automatic pagination behavior

### Phase 6: Categories & Settings
1. Implement CategoriesViewModel with tree building logic
2. Implement CategoriesView with collapsible groups (glass cards)
3. Implement CategoryGroupCard component with glass
4. Implement nav bar trailing add button (glass + interactive, NOT FAB)
5. Implement SettingsView with Form (disconnect, server info, version)
6. Implement CategoryEditViewModel with validation
7. Implement CategoryEditView using SwiftUI Form (automatic glass + HIG compliance)
8. Use DatePicker with `.graphical` style for project dates (iOS 26 native)
9. Test create/edit flows for all budget modes
10. Add haptics for save success and disconnect action

### Phase 7: Polish
1. Add pull-to-refresh on all list views with haptics
2. Add loading indicators for all async operations
3. Add error recovery actions (retry buttons)
4. Refine spacing, colors, typography for Liquid Glass aesthetic
5. Test on multiple iPhone sizes (adjust glass corner radii)
6. Test light/dark mode appearance with glass
7. Test Reduce Transparency accessibility setting
8. Verify haptics respect system settings
9. Profile glass rendering performance with Instruments 26

### Phase 8: Testing and Refinement
1. Write unit tests for all ViewModels
2. Write unit tests for formatters and utilities
3. Manual test all error paths
4. Test all haptic feedback points
5. Fix bugs discovered during testing
6. Performance profiling for list scrolling with glass
7. Verify no glass z-fighting issues

## Custom vs Native Components

### All Native SwiftUI Components with Liquid Glass
The entire app uses only SwiftUI primitives with glass enhancements:
- `List` and `LazyVStack` for scrollable lists (content, no glass)
- `NavigationStack` for navigation (automatic glass nav bar)
- `TabView` for bottom tabs (automatic Liquid Glass styling)
- `TextField` and `SecureField` for text input
- `Button` for actions (automatic glass on standard buttons)
- Card views using `.glassEffect()` modifier on VStack/ZStack containers
- `Badge` using native badge modifier on tab items
- Bottom sheets using `.sheet` modifier (automatic glass backdrop)
- Segmented controls using `Picker` with `.segmentedPickerStyle()` (automatic glass)
- Progress indicators using `ProgressView`

### Custom Component: Spend Bar
Custom view using SwiftUI Canvas for drawing:
- Background track: ZStack with RoundedRectangle + `.glassEffect(.clear)`
- Filled portion: RoundedRectangle with solid color (no glass for readability)
- Budget mark line: Rectangle with opaque color
- No UIKit drop-down required

All other components use native SwiftUI + Liquid Glass modifiers.

## Answered Questions

### 1. iOS Version Target
**Answer**: iOS 26.0 minimum (current version as of March 2026).
- Enables latest Liquid Glass APIs
- Automatic glass styling for system components
- Still maintains good compatibility (iOS 26.0+ devices from 2025 onward)

### 2. Category Search
**Answer**: Client-side filtering only.
- Backend has no category search endpoint
- Filter categories array in ViewModel based on search text
- Search applies to category name only

### 3. Transaction Search
**Answer**: Backend search via API query parameter.
- Endpoint: `GET /api/transactions?search={query}`
- Backend performs case-insensitive substring match on merchant and description
- No client-side filtering needed

### 4. Pagination
**Answer**: Automatic pagination on scroll (iOS-native UX).
- Request: `?limit=50&offset=0`
- Use `.onAppear` on last visible List item to trigger next page load
- Show `ProgressView` at bottom during fetch
- Increment offset by limit on each automatic load
- No "Load More" button (Android pattern, not iOS)
- Note: Cursor-based migration tracked in TODO.md but not implemented yet

### 5. Background Categorization
**Answer**: No polling, rely on pull-to-refresh.
- Trigger job via `POST /api/jobs/categorize` (returns 202 immediately)
- User manually pulls-to-refresh to see results
- Haptic feedback on refresh completion
- Simpler than polling, matches mobile usage patterns

### 6. Logout Placement
**Answer**: Use Settings/About screen (idiomatic iOS pattern).
- **CORRECTION**: Don't match Android top-bar logout
- iOS 26 pattern: add fourth tab "Settings" or use sheet presentation from any tab
- Settings tab contains: server URL (read-only), disconnect button, app version
- More discoverable than toolbar button, follows iOS conventions
- Alternative: Use menu button in nav bar that presents sheet with settings

### 7. Dynamic Color
**Answer**: Use fixed palette (no dynamic color extraction).
- iOS 26 Liquid Glass works with semantic colors
- Define fixed color palette that works with glass tinting
- Simpler than Material You color extraction
- Ensures consistent branding across sessions

### 8. Haptics
**Answer**: Yes, add haptic feedback for key interactions.
- Category assignment: medium impact
- Pull-to-refresh completion: light impact
- Button taps on glass surfaces: light impact
- Tab switching: selection feedback
- Month navigation: selection feedback
- Connection success: success notification
- Errors: error notification
- Respects system haptic settings

### 9. Animations
**Answer**: Use iOS-native transitions (smoother, more "iOS-like").
- NavigationStack push/pop transitions (automatic)
- Tab switching fade (automatic)
- Sheet presentation (automatic with glass backdrop)
- Glass morphing animations (automatic with `.glassEffectID()`)
- Pull-to-refresh bounce (native `.refreshable`)
- Don't match Android animations - embrace iOS conventions

### 10. Config Security
**Answer**: Yes, move API key to Keychain.
- UserDefaults is not secure for sensitive data
- Use native Keychain Services API
- Store both server URL and API key in Keychain
- No third-party keychain wrappers needed
- ConfigStore wraps Keychain access

## Implementation Estimates

### Time Estimates (Conservative)
- Phase 1 (Foundation): 10 hours (includes Keychain + Haptics)
- Phase 2 (Setup): 4 hours
- Phase 3 (Navigation): 4 hours (verify glass tab bar behavior)
- Phase 4 (Dashboard): 14 hours (glass cards + haptics)
- Phase 5 (Transactions): 12 hours (search + glass + haptics)
- Phase 6 (Categories): 10 hours (glass cards + FAB)
- Phase 7 (Polish): 8 hours (Reduce Transparency + performance)
- Phase 8 (Testing): 10 hours (glass rendering + haptics testing)

**Total: ~72 hours** (assume 2 weeks with testing, iteration, and Liquid Glass refinement)

## Liquid Glass Specific Considerations

### Performance
- Glass effects use real-time rendering and can be expensive
- Profile with Instruments 26 before shipping
- Respect Reduce Motion accessibility setting for glass animations
- Use `.glassEffect(.identity)` to conditionally disable glass

### Accessibility
- Detect Reduce Transparency setting:
```swift
@Environment(\.accessibilityReduceTransparency) var reduceTransparency

if reduceTransparency {
    // Use solid backgrounds instead of glass
} else {
    .glassEffect()
}
```
- Ensure 70%+ opacity for all text on glass
- Test with High Contrast mode
- Test with VoiceOver (glass doesn't interfere with screen reader)

### Fallback for iOS 25 and Earlier
Not applicable - app targets iOS 26.0 minimum.

### Design Review
Apple's Liquid Glass is new and evolving. Key design principles:
- **Think in layers**: Content at bottom, glass navigation floating on top
- **Use sparingly**: Only key surfaces (navigation, modals, actions)
- **Maintain hierarchy**: Glass should enhance, not obscure
- **Test extensively**: Glass appearance varies with background content

## Architecture Correctness (Swift 6 + iOS 26 Modern Patterns)

### State Management: @Observable vs ObservableObject
**✅ CORRECT: Use `@Observable` macro** (iOS 17+, Swift 5.9+)
```swift
@Observable
@MainActor
final class ViewModel {
    var state: String = ""  // Automatically observable, no @Published
}

struct MyView: View {
    @State private var viewModel = ViewModel()  // Use @State, not @StateObject
}
```

**❌ INCORRECT: ObservableObject (legacy pattern)**
```swift
class ViewModel: ObservableObject {
    @Published var state: String = ""  // Old pattern
}

struct MyView: View {
    @StateObject private var viewModel = ViewModel()  // Old pattern
}
```

**Why**: `@Observable` provides granular updates (views only re-render when used properties change), no property wrappers needed, fully integrated with Swift concurrency, and is the official replacement for `ObservableObject`.

### Concurrency: MainActor Isolation
**✅ CORRECT: Class-level `@MainActor` for ViewModels**
```swift
@Observable
@MainActor  // Entire class runs on main thread
final class ViewModel {
    func loadData() async {  // Inherits MainActor isolation
        // Can safely update UI properties
    }
}
```

**❌ INCORRECT: Method-level `@MainActor` when entire class is UI-bound**
```swift
@Observable
final class ViewModel {
    @MainActor  // Repetitive if all methods need it
    func loadData() async { }

    @MainActor
    func updateUI() { }
}
```

**Why**: Swift 6.2 "progressive disclosure" - use class-level isolation for UI-bound ViewModels, method-level only for mixed UI/background work.

### Concurrency: APIClient Isolation
**✅ CORRECT: Keep APIClient isolation-free**
```swift
final class APIClient {
    // No @MainActor - flexible isolation
    func fetch() async throws -> Data { }
}
```

**❌ INCORRECT: MainActor on network layer**
```swift
@MainActor  // Over-isolation!
final class APIClient {
    func fetch() async throws -> Data { }
}
```

**Why**: Network layer should remain isolation-free for flexibility. ViewModels (which are `@MainActor`) can call isolation-free async functions, but reverse doesn't work.

### Navigation: Routes Type Constraints
**✅ CORRECT: Hashable routes**
```swift
enum DashboardRoute: Hashable {  // Hashable required for NavigationStack
    case detail(id: String)
}
```

**❌ INCORRECT: Codable routes (unnecessary)**
```swift
enum DashboardRoute: Hashable, Codable {  // Codable not needed unless persisting
    case detail(id: String)
}
```

**Why**: NavigationStack requires `Hashable` for navigation paths. `Codable` only needed for state restoration/deep linking, which we don't implement.

### ViewModel Lifecycle: @State Initialization
**✅ CORRECT: Initialize in View's init**
```swift
struct DashboardView: View {
    @State private var viewModel: DashboardViewModel

    init(apiClient: APIClient) {
        _viewModel = State(initialValue: DashboardViewModel(apiClient: apiClient))
    }
}
```

**❌ INCORRECT: Inline initialization (recreates on every view rebuild)**
```swift
struct DashboardView: View {
    @State private var viewModel = DashboardViewModel()  // Recreated on rebuild!
}
```

**Why**: `@State` with `@Observable` differs from `@StateObject` - it doesn't preserve across view rebuilds unless initialized in `init`.

### Import Hygiene: ViewModels
**✅ CORRECT: Only import Foundation**
```swift
import Foundation  // No SwiftUI import

@Observable
@MainActor
final class ViewModel {
    // Testable, no SwiftUI dependency
}
```

**❌ INCORRECT: Import SwiftUI in ViewModel**
```swift
import SwiftUI  // Coupling to UI framework

@Observable
@MainActor
final class ViewModel { }
```

**Why**: ViewModels should only import Foundation to remain testable and decoupled from SwiftUI.

### Async Task Lifecycle: .task vs .onAppear
**✅ CORRECT: Use `.task` for async loading**
```swift
List(items) { item in
    Text(item.name)
}
.task {
    await viewModel.load()  // Cancels automatically on view disappear
}
```

**❌ INCORRECT: Use `.onAppear` with Task**
```swift
List(items) { item in
    Text(item.name)
}
.onAppear {
    Task {  // Doesn't auto-cancel, can leak
        await viewModel.load()
    }
}
```

**Why**: `.task` modifier automatically cancels the task when the view disappears. `.onAppear` with manual `Task` doesn't auto-cancel.

### Actor Usage: When NOT to Use Actors
**✅ CORRECT: Use `@MainActor` class for ViewModels**
```swift
@Observable
@MainActor
final class ViewModel { }  // Class, not actor
```

**❌ INCORRECT: Use actor for ViewModel**
```swift
@Observable
actor ViewModel { }  // Actor isolation incompatible with @Observable
```

**Why**: `@Observable` requires a class. Actors are for shared mutable state, not UI-bound ViewModels. Use `@MainActor` class instead.

## iOS-Idiomatic Corrections (Android Pattern Rejections)

The following Android patterns were **explicitly rejected** in favor of idiomatic iOS 26 approaches:

### 1. Material FAB → iOS Toolbar/Nav Bar Button
**Android**: Circular floating action button (FAB) in bottom-right corner
**iOS 26**: Use nav bar trailing button OR bottom toolbar (never both with tab bar)
- Apple HIG: "Toolbar OR tab bar, not both"
- Since app uses tab bar, add actions go in nav bar trailing slot
- Bottom toolbar only for apps without tab bar (e.g., Journal, Reminders)

### 2. Custom Form Layout → SwiftUI Form
**Android**: Custom VStack/Column layouts for forms with manual spacing
**iOS 26**: Native SwiftUI Form container
- Automatic Liquid Glass styling when compiled with Xcode 26
- Follows HIG spacing, alignment, grouping
- Platform-appropriate styling (iOS grouped style, macOS different)
- Accessibility built-in

### 3. Three-Tab Layout → Four-Tab Layout
**Android**: Three bottom tabs (Dashboard, Transactions, Categories)
**iOS 26**: Four tabs with Settings as fourth tab
- iOS convention: settings/preferences in dedicated tab
- More discoverable than hidden menu
- Balances tab bar layout (even number preferred)

### 4. Top-Bar Logout → Settings Tab Disconnect
**Android**: Logout button in top navigation bar
**iOS 26**: Disconnect action in Settings tab
- Follows iOS convention of settings in dedicated area
- Settings tab contains: server URL (read-only), disconnect button, app version, logs link
- More intuitive for iOS users

### 5. DatePicker Style
**Android**: Material date picker dialog
**iOS 26**: Native DatePicker with `.graphical` style
- iOS 26 enhanced graphical date picker
- Inline calendar view, no modal dialog
- Matches iOS Calendar app UX

### 6. Pagination "Load More" Button → Automatic iOS Pagination
**Android**: Explicit "Load More" button at list bottom
**iOS 26**: Automatic loading on scroll (iOS standard)
- **Decision**: Use `.onAppear` on last visible item to trigger next page load automatically
- No explicit button - iOS users expect automatic pagination
- Show loading indicator at bottom during fetch
- Backend already supports offset pagination, just increment automatically
- More iOS-native UX (Safari, Messages, Photos all use automatic pagination)

### 7. In-Screen Tab Selector → Segmented Control
**Android**: Material `PrimaryTabRow` component for Monthly/Annual/Projects switching
**iOS 26**: SwiftUI `Picker` with `.segmentedPickerStyle()`
- Segmented control is idiomatic iOS for filtering/switching views within a screen
- Tab bar is for app-level navigation, segmented control for in-screen view switching
- Automatic Liquid Glass styling when compiled with Xcode 26

### 8. CategoryDetailView Navigation → Sheet Presentation
**Android**: Push navigation (back button in nav bar)
**iOS 26**: Modal sheet presentation
- Push is for hierarchical navigation (Settings > Account > Edit Profile)
- Modal sheets are for temporary, focused tasks (viewing category detail, filtering)
- Category detail is a self-contained view, not hierarchical drill-down
- Dismissible with swipe gesture (iOS-native interaction)
- Clearer visual separation from dashboard

## Final Notes

This plan represents a complete native iOS 26 reimplementation with **zero shared code** and full adoption of Apple's **Liquid Glass design system**. The app will feel completely native to iOS users while maintaining feature parity with the Android app.

Every decision prioritizes using **native iOS 26 APIs and Liquid Glass primitives**. Where the Android app uses Material Design components, this app uses iOS equivalents with Liquid Glass styling:
- Material cards → Glass cards with `.glassEffect()`
- ~~Material FAB~~ → **Nav bar trailing button** (idiomatic iOS, not bottom-right FAB)
- Material bottom sheets → SwiftUI sheets with automatic glass backdrop
- Material segmented buttons → SwiftUI Picker with `.segmentedPickerStyle()` (automatic glass)
- ~~Material PrimaryTabRow~~ → **Picker with segmented style** (in-screen view switching)
- Material navigation → NavigationStack with automatic glass nav bar
- Material tab bar → TabView with automatic Liquid Glass capsule tabs (4 tabs, not 3)
- Material form layouts → SwiftUI Form container (automatic HIG styling + glass)
- Material date picker → DatePicker with `.graphical` style (iOS 26 inline calendar)
- Material logout in toolbar → Settings tab with disconnect action
- ~~Push navigation for detail views~~ → **Sheet presentation** (category detail, not hierarchical)
- ~~"Load More" button~~ → **Automatic pagination** on scroll (iOS standard)

The absence of third-party dependencies ensures long-term maintainability. The pure SwiftUI + Liquid Glass approach means the app automatically benefits from future iOS improvements to the glass rendering system.

**Liquid Glass transforms the app from a utilitarian budget tool into a delightful, tactile experience** that feels at home in the iOS 26 ecosystem. The translucent materials, interactive haptics, and fluid animations create an app that's a joy to use daily.

