package com.budget.shared.viewmodel

import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetMonth
import com.budget.shared.api.BudgetStatus
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryRequest
import com.budget.shared.api.PaceIndicator
import com.budget.shared.api.StatusResponse
import com.budget.shared.api.Transaction
import com.budget.shared.api.TransactionEntry
import com.budget.shared.api.TransactionPage
import com.budget.shared.repository.BudgetRepository
import com.budget.shared.repository.DashboardData
import com.budget.shared.repository.InvalidationEvent
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
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
    val repo =
        FakeDashboardRepository(dashboardResults = mapOf(null to makeDashboardData("month-1")))
    val vm = DashboardViewModel(repo)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertNull(state.error)
    assertEquals("month-1", state.currentMonth?.id)
  }

  @Test
  fun monthNavigationUsesCacheOnSecondVisit() = runTest {
    val month1 = makeDashboardData("month-1", nextMonthId = "month-2")
    val month2 = makeDashboardData("month-2", prevMonthId = "month-1")
    val repo =
        FakeDashboardRepository(
            dashboardResults = mapOf(null to month1, "month-1" to month1, "month-2" to month2)
        )
    val vm = DashboardViewModel(repo)
    assertEquals("month-1", vm.uiState.value.currentMonth?.id)

    // Navigate to month-2 (first visit → fetches)
    vm.goToNextMonth()
    assertEquals("month-2", vm.uiState.value.currentMonth?.id)
    val fetchCountAfterFirst = repo.dashboardFetchCount

    // Navigate back to month-1 (should use cache, no new fetch)
    vm.goToPreviousMonth()
    assertEquals("month-1", vm.uiState.value.currentMonth?.id)
    assertEquals(
        fetchCountAfterFirst,
        repo.dashboardFetchCount,
        "Expected cache hit, but a new fetch was made",
    )
  }

  @Test
  fun cachedMonthNavigationDoesNotShowLoading() = runTest {
    val month1 = makeDashboardData("month-1", nextMonthId = "month-2")
    val month2 = makeDashboardData("month-2", prevMonthId = "month-1")
    val repo =
        FakeDashboardRepository(
            dashboardResults = mapOf(null to month1, "month-1" to month1, "month-2" to month2)
        )
    val vm = DashboardViewModel(repo)

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
    val repo =
        FakeDashboardRepository(dashboardResults = mapOf(null to makeDashboardData("month-1")))
    val vm = DashboardViewModel(repo)

    vm.selectCategory("cat-1")
    assertEquals("cat-1", vm.uiState.value.selectedCategoryId)
  }

  @Test
  fun selectSameCategoryTogglesOff() = runTest {
    val repo =
        FakeDashboardRepository(dashboardResults = mapOf(null to makeDashboardData("month-1")))
    val vm = DashboardViewModel(repo)

    vm.selectCategory("cat-1")
    assertEquals("cat-1", vm.uiState.value.selectedCategoryId)

    vm.selectCategory("cat-1")
    assertNull(vm.uiState.value.selectedCategoryId)
  }

  @Test
  fun monthNavigationClearsSelectedCategory() = runTest {
    val month1 = makeDashboardData("month-1", nextMonthId = "month-2")
    val month2 = makeDashboardData("month-2", prevMonthId = "month-1")
    val repo =
        FakeDashboardRepository(
            dashboardResults = mapOf(null to month1, "month-1" to month1, "month-2" to month2)
        )
    val vm = DashboardViewModel(repo)

    vm.selectCategory("cat-1")
    assertEquals("cat-1", vm.uiState.value.selectedCategoryId)

    vm.goToNextMonth()
    assertNull(vm.uiState.value.selectedCategoryId)
  }

  @Test
  fun tabSwitchClearsSelectedCategory() = runTest {
    val repo =
        FakeDashboardRepository(dashboardResults = mapOf(null to makeDashboardData("month-1")))
    val vm = DashboardViewModel(repo)

    vm.selectCategory("cat-1")
    vm.selectTab(BudgetMode.ANNUAL)
    assertNull(vm.uiState.value.selectedCategoryId)
  }

  @Test
  fun fetchErrorSetsErrorState() = runTest {
    val repo = FakeDashboardRepository(dashboardError = "Network failure")
    val vm = DashboardViewModel(repo)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertEquals("Network failure", state.error)
  }

  @Test
  fun prefetchesAdjacentMonths() = runTest {
    val month1 = makeDashboardData("month-1", nextMonthId = "month-2")
    val month2 = makeDashboardData("month-2", prevMonthId = "month-1")
    val repo =
        FakeDashboardRepository(
            dashboardResults = mapOf(null to month1, "month-1" to month1, "month-2" to month2)
        )
    val vm = DashboardViewModel(repo)

    // After initial load of month-1, month-2 should be prefetched.
    // Total fetches: 1 (initial) + 1 (prefetch month-2) = 2
    // Note: month-1 is index 0 so no prev to prefetch.
    assertTrue(repo.dashboardFetchCount >= 2, "Expected prefetch of adjacent month")

    // Navigate to month-2: should be cached (no additional fetch)
    val countBefore = repo.dashboardFetchCount
    vm.goToNextMonth()
    assertEquals("month-2", vm.uiState.value.currentMonth?.id)
    assertEquals(countBefore, repo.dashboardFetchCount, "month-2 should have been prefetched")
  }

  @Test
  fun invalidationEventClearsCacheAndReloads() = runTest {
    val repo =
        FakeDashboardRepository(dashboardResults = mapOf(null to makeDashboardData("month-1")))
    val vm = DashboardViewModel(repo)
    assertEquals("month-1", vm.uiState.value.currentMonth?.id)

    val countBefore = repo.dashboardFetchCount
    repo.emitInvalidation(InvalidationEvent.TRANSACTIONS)
    assertTrue(repo.dashboardFetchCount > countBefore, "Expected re-fetch after invalidation")
  }

  @Test
  fun unbudgetedSpentFlowsThroughToUiState() = runTest {
    val unbudgetedTxns =
        listOf(
            TransactionEntry(
                id = "txn-ub-1",
                categoryId = "cat-none",
                amount = 30.0,
                merchantName = "Coffee Shop",
                postedDate = "2026-02-20",
            ),
            TransactionEntry(
                id = "txn-ub-2",
                categoryId = "cat-none",
                amount = 15.0,
                merchantName = "Bakery",
                postedDate = "2026-02-22",
            ),
        )
    val data =
        makeDashboardData(
            "month-1",
            unbudgetedSpent = 45.0,
            unbudgetedTransactions = unbudgetedTxns,
        )
    val repo = FakeDashboardRepository(dashboardResults = mapOf(null to data))
    val vm = DashboardViewModel(repo)

    val state = vm.uiState.value
    assertEquals(45.0, state.unbudgetedSpent)
    assertEquals(2, state.unbudgetedTransactions.size)
    // Unbudgeted transactions should be merged into monthlyTransactions
    assertTrue(
        state.monthlyTransactions.any { it.id == "txn-ub-1" },
        "Expected unbudgeted transaction in monthlyTransactions",
    )
  }
}

// -- Test doubles -----------------------------------------------------------

private class FakeDashboardRepository(
    private val dashboardResults: Map<String?, DashboardData> = emptyMap(),
    private val dashboardError: String? = null,
) : BudgetRepository {

  var dashboardFetchCount = 0
    private set

  private val _invalidationEvents = MutableSharedFlow<InvalidationEvent>(extraBufferCapacity = 1)
  override val invalidationEvents: SharedFlow<InvalidationEvent> =
      _invalidationEvents.asSharedFlow()

  suspend fun emitInvalidation(event: InvalidationEvent) {
    _invalidationEvents.emit(event)
  }

  override suspend fun getDashboardData(monthId: String?): DashboardData {
    dashboardFetchCount++
    if (dashboardError != null) throw RuntimeException(dashboardError)
    return dashboardResults[monthId] ?: throw RuntimeException("No data for monthId=$monthId")
  }

  override suspend fun getTransactions(limit: Int, offset: Int, categoryId: String?) =
      TransactionPage(emptyList(), 0, limit, offset)

  override suspend fun getTransaction(id: String): Transaction = throw NotImplementedError()

  override suspend fun getCategories(): List<Category> = emptyList()

  override suspend fun categorizeTransaction(transactionId: String, categoryId: String) = true

  override suspend fun uncategorizeTransaction(transactionId: String) = true

  override suspend fun createCategory(request: CategoryRequest): Category =
      throw NotImplementedError()

  override suspend fun updateCategory(id: String, request: CategoryRequest): Category =
      throw NotImplementedError()
}

private fun makeDashboardData(
    monthId: String,
    prevMonthId: String? = null,
    nextMonthId: String? = null,
    unbudgetedSpent: Double = 0.0,
    unbudgetedTransactions: List<TransactionEntry> = emptyList(),
): DashboardData {
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
                      paceDelta = -83.33,
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
          unbudgetedSpent = unbudgetedSpent,
          unbudgetedTransactions = unbudgetedTransactions,
      )
  return DashboardData(status = status, months = months)
}
