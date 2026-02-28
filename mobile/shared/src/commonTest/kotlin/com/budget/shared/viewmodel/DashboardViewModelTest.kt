package com.budget.shared.viewmodel

import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetMonth
import com.budget.shared.api.BudgetStatus
import com.budget.shared.api.PaceIndicator
import com.budget.shared.api.StatusResponse
import com.budget.shared.api.TransactionEntry
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain

@OptIn(ExperimentalCoroutinesApi::class)
class DashboardViewModelTest {

  private val testDispatcher = UnconfinedTestDispatcher()

  @BeforeTest
  fun setUp() {
    Dispatchers.setMain(testDispatcher)
  }

  @AfterTest
  fun tearDown() {
    Dispatchers.resetMain()
  }

  @Test
  fun initialLoadFetchesDataAndPopulatesState() = runTest {
    val fetcher = FakeDashboardFetcher(mapOf(null to makeFetchResult("month-1")))
    val vm = DashboardViewModel("https://example.com", "key", fetcher)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertNull(state.error)
    assertEquals("month-1", state.currentMonth?.id)
  }

  @Test
  fun monthNavigationUsesCacheOnSecondVisit() = runTest {
    val month1 = makeFetchResult("month-1", nextMonthId = "month-2")
    val month2 = makeFetchResult("month-2", prevMonthId = "month-1")
    val fetcher =
        FakeDashboardFetcher(mapOf(null to month1, "month-1" to month1, "month-2" to month2))
    val vm = DashboardViewModel("https://example.com", "key", fetcher)
    assertEquals("month-1", vm.uiState.value.currentMonth?.id)

    // Navigate to month-2 (first visit → fetches)
    vm.goToNextMonth()
    assertEquals("month-2", vm.uiState.value.currentMonth?.id)
    val fetchCountAfterFirst = fetcher.fetchCount

    // Navigate back to month-1 (should use cache, no new fetch)
    vm.goToPreviousMonth()
    assertEquals("month-1", vm.uiState.value.currentMonth?.id)
    assertEquals(
        fetchCountAfterFirst,
        fetcher.fetchCount,
        "Expected cache hit, but a new fetch was made",
    )
  }

  @Test
  fun cachedMonthNavigationDoesNotShowLoading() = runTest {
    val month1 = makeFetchResult("month-1", nextMonthId = "month-2")
    val month2 = makeFetchResult("month-2", prevMonthId = "month-1")
    val fetcher =
        FakeDashboardFetcher(mapOf(null to month1, "month-1" to month1, "month-2" to month2))
    val vm = DashboardViewModel("https://example.com", "key", fetcher)

    // Visit month-2 so it gets cached
    vm.goToNextMonth()
    assertEquals("month-2", vm.uiState.value.currentMonth?.id)

    // Go back to month-1 (cached from initial load)
    vm.goToPreviousMonth()
    // State should not have gone through loading=true
    assertFalse(vm.uiState.value.loading)
  }

  @Test
  fun selectCategorySetsSelectedId() = runTest {
    val fetcher = FakeDashboardFetcher(mapOf(null to makeFetchResult("month-1")))
    val vm = DashboardViewModel("https://example.com", "key", fetcher)

    vm.selectCategory("cat-1")
    assertEquals("cat-1", vm.uiState.value.selectedCategoryId)
  }

  @Test
  fun selectSameCategoryTogglesOff() = runTest {
    val fetcher = FakeDashboardFetcher(mapOf(null to makeFetchResult("month-1")))
    val vm = DashboardViewModel("https://example.com", "key", fetcher)

    vm.selectCategory("cat-1")
    assertEquals("cat-1", vm.uiState.value.selectedCategoryId)

    vm.selectCategory("cat-1")
    assertNull(vm.uiState.value.selectedCategoryId)
  }

  @Test
  fun monthNavigationClearsSelectedCategory() = runTest {
    val month1 = makeFetchResult("month-1", nextMonthId = "month-2")
    val month2 = makeFetchResult("month-2", prevMonthId = "month-1")
    val fetcher =
        FakeDashboardFetcher(mapOf(null to month1, "month-1" to month1, "month-2" to month2))
    val vm = DashboardViewModel("https://example.com", "key", fetcher)

    vm.selectCategory("cat-1")
    assertEquals("cat-1", vm.uiState.value.selectedCategoryId)

    vm.goToNextMonth()
    assertNull(vm.uiState.value.selectedCategoryId)
  }

  @Test
  fun tabSwitchClearsSelectedCategory() = runTest {
    val fetcher = FakeDashboardFetcher(mapOf(null to makeFetchResult("month-1")))
    val vm = DashboardViewModel("https://example.com", "key", fetcher)

    vm.selectCategory("cat-1")
    vm.selectTab(BudgetMode.ANNUAL)
    assertNull(vm.uiState.value.selectedCategoryId)
  }

  @Test
  fun fetchErrorSetsErrorState() = runTest {
    val fetcher = FakeDashboardFetcher(error = "Network failure")
    val vm = DashboardViewModel("https://example.com", "key", fetcher)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertEquals("Network failure", state.error)
  }

  @Test
  fun prefetchesAdjacentMonths() = runTest {
    val month1 = makeFetchResult("month-1", nextMonthId = "month-2")
    val month2 = makeFetchResult("month-2", prevMonthId = "month-1")
    val fetcher =
        FakeDashboardFetcher(mapOf(null to month1, "month-1" to month1, "month-2" to month2))
    val vm = DashboardViewModel("https://example.com", "key", fetcher)

    // After initial load of month-1, month-2 should be prefetched.
    // Total fetches: 1 (initial) + 1 (prefetch month-2) = 2
    // Note: month-1 is index 0 so no prev to prefetch.
    assertTrue(fetcher.fetchCount >= 2, "Expected prefetch of adjacent month")

    // Navigate to month-2: should be cached (no additional fetch)
    val countBefore = fetcher.fetchCount
    vm.goToNextMonth()
    assertEquals("month-2", vm.uiState.value.currentMonth?.id)
    assertEquals(countBefore, fetcher.fetchCount, "month-2 should have been prefetched")
  }
}

// -- Test doubles -----------------------------------------------------------

private class FakeDashboardFetcher(
    private val results: Map<String?, FetchResult> = emptyMap(),
    private val error: String? = null,
) : DashboardFetcher {
  var fetchCount = 0
    private set

  override suspend fun fetch(serverUrl: String, apiKey: String, monthId: String?): FetchResult {
    fetchCount++
    if (error != null) throw RuntimeException(error)
    return results[monthId] ?: throw RuntimeException("No data for monthId=$monthId")
  }
}

private fun makeFetchResult(
    monthId: String,
    prevMonthId: String? = null,
    nextMonthId: String? = null,
): FetchResult {
  val months = buildList {
    if (prevMonthId != null) add(BudgetMonth(id = prevMonthId, startDate = "2026-01-28"))
    add(BudgetMonth(id = monthId, startDate = "2026-02-28"))
    if (nextMonthId != null) add(BudgetMonth(id = nextMonthId, startDate = "2026-03-28"))
  }
  val status =
      StatusResponse(
          month = BudgetMonth(id = monthId, startDate = "2026-02-28"),
          statuses =
              listOf(
                  BudgetStatus(
                      categoryId = "cat-1",
                      categoryName = "Groceries",
                      budgetAmount = 500.0,
                      spent = 250.0,
                      remaining = 250.0,
                      timeLeft = 10,
                      pace = PaceIndicator.UNDER_BUDGET,
                      budgetMode = BudgetMode.MONTHLY,
                  ),
              ),
          monthlyTransactions =
              listOf(
                  TransactionEntry(
                      id = "txn-1",
                      categoryId = "cat-1",
                      amount = 45.0,
                      merchantName = "Supermarket",
                      postedDate = "2026-02-25",
                  ),
              ),
      )
  return FetchResult(status = status, months = months)
}
