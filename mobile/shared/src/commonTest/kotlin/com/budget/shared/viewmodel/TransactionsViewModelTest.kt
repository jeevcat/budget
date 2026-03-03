package com.budget.shared.viewmodel

import com.budget.shared.api.Category
import com.budget.shared.api.CategoryMethod
import com.budget.shared.api.CategoryName
import com.budget.shared.api.CategoryRequest
import com.budget.shared.api.Transaction
import com.budget.shared.api.TransactionPage
import com.budget.shared.repository.BudgetRepository
import com.budget.shared.repository.DashboardData
import com.budget.shared.repository.InvalidationEvent
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
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
class TransactionsViewModelTest {

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
  fun buildDisplayCategoriesExcludesParentsShowsOnlyLeaves() {
    val cats =
        listOf(
            Category(id = "1", name = catName("Groceries")),
            Category(id = "2", name = catName("Transport")),
            Category(id = "3", name = catName("Bus"), parentId = "2"),
            Category(id = "4", name = catName("Aldi"), parentId = "1"),
            Category(id = "5", name = catName("Train"), parentId = "2"),
        )

    val result = TransactionsViewModel.buildDisplayCategories(cats)

    // Parent categories (Groceries, Transport) are excluded; only leaves shown
    assertEquals(3, result.size)
    // Children of Groceries (alphabetical)
    assertEquals("Aldi", result[0].name)
    assertEquals("Groceries", result[0].parentName)
    assertEquals(1, result[0].depth)
    // Children of Transport (alphabetical)
    assertEquals("Bus", result[1].name)
    assertEquals("Transport", result[1].parentName)
    assertEquals(1, result[1].depth)
    assertEquals("Train", result[2].name)
    assertEquals("Transport", result[2].parentName)
  }

  @Test
  fun buildDisplayCategoriesIncludesLeafRoots() {
    val cats =
        listOf(
            Category(id = "1", name = catName("Rent")),
            Category(id = "2", name = catName("Food")),
        )

    val result = TransactionsViewModel.buildDisplayCategories(cats)

    // Root categories without children are leaf categories — they're selectable
    assertEquals(2, result.size)
    assertEquals("Food", result[0].name) // alphabetical
    assertEquals("Rent", result[1].name)
    assertEquals(0, result[0].depth)
    assertNull(result[0].parentName)
  }

  @Test
  fun buildDisplayCategoriesMixesLeafRootsAndChildren() {
    val cats =
        listOf(
            Category(id = "1", name = catName("Tax")),
            Category(id = "2", name = catName("Food")),
            Category(id = "3", name = catName("Coffee"), parentId = "2"),
            Category(id = "4", name = catName("Groceries"), parentId = "2"),
        )

    val result = TransactionsViewModel.buildDisplayCategories(cats)

    // Food is a parent → excluded. Tax is a leaf root → included.
    assertEquals(3, result.size)
    assertEquals("Coffee", result[0].name)
    assertEquals("Food", result[0].parentName)
    assertEquals("Groceries", result[1].name)
    assertEquals("Food", result[1].parentName)
    assertEquals("Tax", result[2].name)
    assertEquals(0, result[2].depth)
    assertNull(result[2].parentName)
  }

  @Test
  fun displayCategoryDisplayNameIncludesParent() {
    val dc = DisplayCategory(id = "1", name = "Bus", parentName = "Transport", depth = 1)
    assertEquals("Transport > Bus", dc.displayName)
  }

  @Test
  fun displayCategoryDisplayNameWithoutParent() {
    val dc = DisplayCategory(id = "1", name = "Groceries")
    assertEquals("Groceries", dc.displayName)
  }

  @Test
  fun initialLoadFetchesUncategorizedTransactions() = runTest {
    val txn =
        Transaction(
            id = "t1",
            accountId = "a1",
            amount = "-50.00",
            merchantName = "Supermarket",
            postedDate = "2026-02-28",
        )
    val repo =
        FakeTransactionsRepository(
            transactions =
                TransactionPage(
                    items = listOf(txn),
                    total = 1,
                    limit = 200,
                    offset = 0,
                ),
            categories = listOf(Category(id = "c1", name = catName("Food"))),
        )

    val vm = TransactionsViewModel(repo)
    val state = vm.uiState.value

    assertEquals(false, state.loading)
    assertEquals(1, state.transactions.size)
    assertEquals("Supermarket", state.transactions[0].merchantName)
    assertEquals(1, state.total)
    assertEquals(1, state.categories.size)
    assertEquals("Food", state.categories[0].name)
  }

  @Test
  fun categorizeUpdatesSelectedTransactionInPlace() = runTest {
    val txn =
        Transaction(
            id = "t1",
            accountId = "a1",
            amount = "-50.00",
            merchantName = "Supermarket",
            postedDate = "2026-02-28",
        )
    val repo =
        FakeTransactionsRepository(
            transactions =
                TransactionPage(
                    items = listOf(txn),
                    total = 1,
                    limit = 200,
                    offset = 0,
                ),
            categories = listOf(Category(id = "c1", name = catName("Food"))),
        )

    val vm = TransactionsViewModel(repo)

    // Select the transaction
    vm.selectTransaction(txn)
    assertEquals(txn, vm.uiState.value.selectedTransaction)

    // Categorize it
    vm.categorize("c1")

    val state = vm.uiState.value
    val selected = assertNotNull(state.selectedTransaction)
    assertEquals("c1", selected.categoryId)
    assertEquals(CategoryMethod.MANUAL, selected.categoryMethod)
    assertEquals(0, state.total)
    assertTrue(state.transactions.isEmpty())
  }

  @Test
  fun categorySearchFilters() = runTest {
    val repo =
        FakeTransactionsRepository(
            transactions = TransactionPage(items = emptyList(), total = 0, limit = 200, offset = 0),
            categories =
                listOf(
                    Category(id = "c1", name = catName("Food")),
                    Category(id = "c2", name = catName("Transport")),
                    Category(id = "c3", name = catName("Rent")),
                ),
        )

    val vm = TransactionsViewModel(repo)

    vm.updateCategorySearch("foo")
    val filtered = vm.filteredCategories()

    assertEquals(1, filtered.size)
    assertEquals("Food", filtered[0].name)
  }

  @Test
  fun fetchErrorSetsErrorState() = runTest {
    val repo = FakeTransactionsRepository(shouldThrow = true)

    val vm = TransactionsViewModel(repo)
    val state = vm.uiState.value

    assertEquals(false, state.loading)
    assertEquals("Test error", state.error)
    assertTrue(state.transactions.isEmpty())
  }

  @Test
  fun selectTransactionByIdUsesDetailLoading() = runTest {
    val txn =
        Transaction(
            id = "t1",
            accountId = "a1",
            amount = "-50.00",
            merchantName = "Supermarket",
            postedDate = "2026-02-28",
        )
    val repo =
        FakeTransactionsRepository(
            transactions =
                TransactionPage(
                    items = listOf(txn),
                    total = 1,
                    limit = 200,
                    offset = 0,
                ),
            categories = listOf(Category(id = "c1", name = catName("Food"))),
        )

    val vm = TransactionsViewModel(repo)

    vm.selectTransactionById("t1")

    val state = vm.uiState.value
    assertEquals(false, state.detailLoading)
    assertEquals(false, state.loading)
    val selected = assertNotNull(state.selectedTransaction)
    assertEquals("t1", selected.id)
  }

  @Test
  fun categorizeFailureSetsError() = runTest {
    val txn =
        Transaction(
            id = "t1",
            accountId = "a1",
            amount = "-50.00",
            merchantName = "Supermarket",
            postedDate = "2026-02-28",
        )
    val repo =
        FakeTransactionsRepository(
            transactions =
                TransactionPage(
                    items = listOf(txn),
                    total = 1,
                    limit = 200,
                    offset = 0,
                ),
            categories = emptyList(),
            categorizeResult = false,
        )

    val vm = TransactionsViewModel(repo)
    vm.selectTransaction(txn)
    vm.categorize("c1")

    val state = vm.uiState.value
    assertEquals("Failed to categorize transaction", state.error)
    assertEquals(false, state.categorizing)
  }
}

// -- Helpers ------------------------------------------------------------

private fun catName(name: String): CategoryName = CategoryName.of(name).getOrThrow()

// -- Test doubles -------------------------------------------------------

private class FakeTransactionsRepository(
    private val transactions: TransactionPage = TransactionPage(emptyList(), 0, 200, 0),
    private val categories: List<Category> = emptyList(),
    private val categorizeResult: Boolean = true,
    private val uncategorizeResult: Boolean = true,
    private val shouldThrow: Boolean = false,
) : BudgetRepository {

  private val _invalidationEvents = MutableSharedFlow<InvalidationEvent>()
  override val invalidationEvents: SharedFlow<InvalidationEvent> =
      _invalidationEvents.asSharedFlow()

  override suspend fun getDashboardData(monthId: String?): DashboardData =
      throw NotImplementedError()

  override suspend fun getTransactions(
      limit: Int,
      offset: Int,
      categoryId: String?,
  ): TransactionPage {
    if (shouldThrow) throw RuntimeException("Test error")
    return transactions
  }

  override suspend fun getTransaction(id: String): Transaction {
    if (shouldThrow) throw RuntimeException("Test error")
    return transactions.items.first { it.id == id }
  }

  override suspend fun getCategories(): List<Category> {
    if (shouldThrow) throw RuntimeException("Test error")
    return categories
  }

  override suspend fun categorizeTransaction(
      transactionId: String,
      categoryId: String,
  ): Boolean = categorizeResult

  override suspend fun uncategorizeTransaction(transactionId: String): Boolean = uncategorizeResult

  override suspend fun createCategory(request: CategoryRequest): Category =
      throw NotImplementedError()

  override suspend fun updateCategory(id: String, request: CategoryRequest): Category =
      throw NotImplementedError()
}
