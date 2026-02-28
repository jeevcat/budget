package com.budget.shared.viewmodel

import com.budget.shared.api.BudgetMode
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryMethod
import com.budget.shared.api.Transaction
import com.budget.shared.api.TransactionPage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

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
    fun buildDisplayCategoriesSortsAlphabeticallyWithChildren() {
        val cats = listOf(
            Category(id = "1", name = "Groceries"),
            Category(id = "2", name = "Transport"),
            Category(id = "3", name = "Bus", parentId = "2"),
            Category(id = "4", name = "Aldi", parentId = "1"),
            Category(id = "5", name = "Train", parentId = "2"),
        )

        val result = TransactionsViewModel.buildDisplayCategories(cats)

        assertEquals(5, result.size)
        // Roots sorted alphabetically: Groceries, Transport
        assertEquals("Groceries", result[0].name)
        assertEquals(0, result[0].depth)
        assertEquals("Aldi", result[1].name)
        assertEquals("Groceries", result[1].parentName)
        assertEquals(1, result[1].depth)
        assertEquals("Transport", result[2].name)
        assertEquals(0, result[2].depth)
        assertEquals("Bus", result[3].name)
        assertEquals("Transport", result[3].parentName)
        assertEquals("Train", result[4].name)
    }

    @Test
    fun buildDisplayCategoriesHandlesNoChildren() {
        val cats = listOf(
            Category(id = "1", name = "Rent"),
            Category(id = "2", name = "Food"),
        )

        val result = TransactionsViewModel.buildDisplayCategories(cats)

        assertEquals(2, result.size)
        assertEquals("Food", result[0].name) // alphabetical
        assertEquals("Rent", result[1].name)
        assertNull(result[0].parentName)
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
        val txn = Transaction(
            id = "t1",
            accountId = "a1",
            amount = "-50.00",
            merchantName = "Supermarket",
            postedDate = "2026-02-28",
        )
        val fetcher = FakeTransactionsFetcher(
            transactions = TransactionPage(
                items = listOf(txn),
                total = 1,
                limit = 200,
                offset = 0,
            ),
            categories = listOf(Category(id = "c1", name = "Food")),
        )

        val vm = TransactionsViewModel("https://example.com", "key", fetcher)
        val state = vm.uiState.value

        assertEquals(false, state.loading)
        assertEquals(1, state.transactions.size)
        assertEquals("Supermarket", state.transactions[0].merchantName)
        assertEquals(1, state.total)
        assertEquals(1, state.categories.size)
        assertEquals("Food", state.categories[0].name)
    }

    @Test
    fun categorizeRemovesTransactionFromList() = runTest {
        val txn = Transaction(
            id = "t1",
            accountId = "a1",
            amount = "-50.00",
            merchantName = "Supermarket",
            postedDate = "2026-02-28",
        )
        val fetcher = FakeTransactionsFetcher(
            transactions = TransactionPage(
                items = listOf(txn),
                total = 1,
                limit = 200,
                offset = 0,
            ),
            categories = listOf(Category(id = "c1", name = "Food")),
        )

        val vm = TransactionsViewModel("https://example.com", "key", fetcher)

        // Select the transaction
        vm.selectTransaction(txn)
        assertEquals(txn, vm.uiState.value.selectedTransaction)

        // Categorize it
        vm.categorize("c1")

        val state = vm.uiState.value
        assertNull(state.selectedTransaction)
        assertEquals(0, state.total)
        assertTrue(state.transactions.isEmpty())
    }

    @Test
    fun categorySearchFilters() = runTest {
        val fetcher = FakeTransactionsFetcher(
            transactions = TransactionPage(items = emptyList(), total = 0, limit = 200, offset = 0),
            categories = listOf(
                Category(id = "c1", name = "Food"),
                Category(id = "c2", name = "Transport"),
                Category(id = "c3", name = "Rent"),
            ),
        )

        val vm = TransactionsViewModel("https://example.com", "key", fetcher)

        vm.updateCategorySearch("foo")
        val filtered = vm.filteredCategories()

        assertEquals(1, filtered.size)
        assertEquals("Food", filtered[0].name)
    }

    @Test
    fun fetchErrorSetsErrorState() = runTest {
        val fetcher = FakeTransactionsFetcher(shouldThrow = true)

        val vm = TransactionsViewModel("https://example.com", "key", fetcher)
        val state = vm.uiState.value

        assertEquals(false, state.loading)
        assertEquals("Test error", state.error)
        assertTrue(state.transactions.isEmpty())
    }

    @Test
    fun categorizeFailureSetsError() = runTest {
        val txn = Transaction(
            id = "t1",
            accountId = "a1",
            amount = "-50.00",
            merchantName = "Supermarket",
            postedDate = "2026-02-28",
        )
        val fetcher = FakeTransactionsFetcher(
            transactions = TransactionPage(
                items = listOf(txn),
                total = 1,
                limit = 200,
                offset = 0,
            ),
            categories = emptyList(),
            categorizeResult = false,
        )

        val vm = TransactionsViewModel("https://example.com", "key", fetcher)
        vm.selectTransaction(txn)
        vm.categorize("c1")

        val state = vm.uiState.value
        assertEquals("Failed to categorize transaction", state.error)
        assertEquals(false, state.categorizing)
    }
}

// -- Test doubles -------------------------------------------------------

private class FakeTransactionsFetcher(
    private val transactions: TransactionPage = TransactionPage(emptyList(), 0, 200, 0),
    private val categories: List<Category> = emptyList(),
    private val categorizeResult: Boolean = true,
    private val uncategorizeResult: Boolean = true,
    private val shouldThrow: Boolean = false,
) : TransactionsFetcher {

    override suspend fun fetchTransactions(
        serverUrl: String,
        apiKey: String,
        limit: Int,
        offset: Int,
        categoryId: String?,
    ): TransactionPage {
        if (shouldThrow) throw RuntimeException("Test error")
        return transactions
    }

    override suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category> {
        if (shouldThrow) throw RuntimeException("Test error")
        return categories
    }

    override suspend fun categorize(
        serverUrl: String,
        apiKey: String,
        transactionId: String,
        categoryId: String,
    ): Boolean = categorizeResult

    override suspend fun uncategorize(
        serverUrl: String,
        apiKey: String,
        transactionId: String,
    ): Boolean = uncategorizeResult
}
